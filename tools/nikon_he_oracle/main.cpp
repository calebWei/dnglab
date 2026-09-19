// Oracle harness: runs the reference Nikon HE decoder standalone and writes the
// decoded CFA as a 16-bit PGM, for validating the Rust port in dnglab.
//
// Usage: nikon_he_oracle <strip.bin> <out.pgm>
//   <strip.bin> = raw NEF strip bytes (StripOffset .. +StripByteCounts),
//                 i.e. the JPEG-XS codestream starting at the SOC marker.
//
// See README.md. The reference C++ is NOT vendored here — it is compiled from a
// local clone of yogthos/LibRaw (branch nikon-he-decoder) pointed to by the
// NIKON_HE_REF_DIR CMake cache variable.

#include "nikon_he_decode.h"
#include "nikon_he_picture_header.h"
#include "nikon_he_iqx_iqp_lut_data.h"
#include "nikon_he_gtli_table.h"

#include <cstdio>
#include <cstdint>
#include <vector>
#include <cstring>

static std::vector<uint8_t> read_file(const char* path) {
  FILE* f = fopen(path, "rb");
  if (!f) { fprintf(stderr, "cannot open %s\n", path); return {}; }
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  std::vector<uint8_t> buf(n > 0 ? (size_t)n : 0);
  if (n > 0 && fread(buf.data(), 1, buf.size(), f) != buf.size()) buf.clear();
  fclose(f);
  return buf;
}

int main(int argc, char** argv) {
  if (argc < 3) { fprintf(stderr, "usage: %s <strip.bin> <out.pgm>\n", argv[0]); return 2; }

  std::vector<uint8_t> strip = read_file(argv[1]);
  if (strip.empty()) { fprintf(stderr, "empty/missing strip\n"); return 2; }

  nikon_he::PictureHeader ph;
  if (!nikon_he::parse_picture_header(strip.data(), strip.size(), ph)) {
    fprintf(stderr, "parse_picture_header failed\n");
    return 3;
  }
  const bool supported = nikon_he::is_supported_picture_header(ph, strip.size());
  fprintf(stderr,
          "PIH: %ux%u comps=%u nbands=%d Hsl=%u Bw=%u Lcod=%u precinct_offset=%zu supported=%d\n",
          ph.hdr_width, ph.hdr_height, ph.comps_num, ph.nbands, ph.Hsl, ph.Bw,
          ph.Lcod, ph.precinct_offset, (int)supported);

  const int W = (int)ph.hdr_width;
  const int H = (int)ph.hdr_height;
  std::vector<uint16_t> bayer((size_t)W * H, 0);

  nikon_he::set_active_picture_header(&ph);
  nikon_he::HeDecodeResult r = nikon_he::decode_nikon_he_image(
      strip.data() + ph.precinct_offset,
      strip.size() - ph.precinct_offset,
      W, H, nikon_he::iqx_iqp_lut(), bayer.data());
  nikon_he::set_active_picture_header(nullptr);

  fprintf(stderr, "decode: success=%d tiles=%d precincts=%d\n",
          (int)r.success, r.tiles_decoded, r.total_precincts);
  if (!r.success) return 4;

  // Write 16-bit big-endian PGM (maxval 65535) to match dnglab's --raw-pixel dump.
  FILE* out = fopen(argv[2], "wb");
  if (!out) { fprintf(stderr, "cannot open output\n"); return 5; }
  fprintf(out, "P5 %d %d 65535\n", W, H);
  std::vector<uint8_t> row((size_t)W * 2);
  for (int y = 0; y < H; ++y) {
    for (int x = 0; x < W; ++x) {
      uint16_t v = bayer[(size_t)y * W + x];
      row[2 * x] = (uint8_t)(v >> 8);
      row[2 * x + 1] = (uint8_t)(v & 0xff);
    }
    fwrite(row.data(), 1, row.size(), out);
  }
  fclose(out);
  fprintf(stderr, "wrote %s\n", argv[2]);
  return 0;
}
