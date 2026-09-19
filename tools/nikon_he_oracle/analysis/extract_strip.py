import struct,sys,os
fn=r"D:\_repos\dnglab\Example Photos\DSC_8070.NEF"
b=open(fn,'rb').read()
end='<' if b[:2]==b'II' else '>'
u16=lambda o:struct.unpack(end+'H',b[o:o+2])[0]
u32=lambda o:struct.unpack(end+'I',b[o:o+4])[0]
def ifd(off):
    n=u16(off);d={}
    for i in range(n):
        e=off+2+i*12;tag=u16(e);typ=u16(e+2);cnt=u32(e+4)
        bl={1:1,2:1,3:2,4:4,5:8}.get(typ,4)*cnt
        vo=e+8 if bl<=4 else u32(e+8)
        d[tag]=vo
    return d
r0=ifd(u32(4))
subs=[u32(r0[330]+k*4) for k in range(0, 8) if True]
# properly read count
n_sub= None
# re-read 330 count
def ifd_full(off):
    n=u16(off);d={}
    for i in range(n):
        e=off+2+i*12;tag=u16(e);typ=u16(e+2);cnt=u32(e+4)
        bl={1:1,2:1,3:2,4:4,5:8}.get(typ,4)*cnt
        vo=e+8 if bl<=4 else u32(e+8)
        d[tag]=(typ,cnt,vo)
    return d
r0=ifd_full(u32(4))
typ,cnt,vo=r0[330]
subs=[u32(vo+k*4) for k in range(cnt)]
for s in subs:
    d=ifd_full(s)
    if 259 in d and u16(d[259][2])==34713 and 262 in d and u16(d[262][2])==32803:
        w=u32(d[256][2]);h=u32(d[257][2]);so=u32(d[273][2]);sb=u32(d[279][2])
        print(f"RAW: {w}x{h} off={so} size={sb}")
        open(r"D:\_repos\_ref_libraw_he\oracle\strip_8070.bin","wb").write(b[so:so+sb])
        print("wrote strip_8070.bin", sb, "bytes; header:", ' '.join(f'{x:02x}' for x in b[so:so+8]))
        break
