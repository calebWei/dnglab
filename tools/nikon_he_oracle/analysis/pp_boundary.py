import glob, os
def analyze(path):
    strip=open(path,"rb").read()
    base=155
    def be(b,o,n):
        v=0
        for i in range(n): v=(v<<8)|b[o+i]
        return v
    total=be(strip,base,3)
    def parse_hdr(o):
        val=be(strip,o,7)
        return (val>>55)&1,(val>>35)&0xFFFFF,(val>>15)&0xFFFFF,val&0x7FFF
    print(f"\n=== {os.path.basename(path)} total={total} Bp={strip[base+3]} Br={strip[base+4]} ===")
    cur=base+12
    for lb in range(8):
        flag,data,gcli,sign=parse_hdr(cur)
        core=data+gcli+sign
        # find sig so that next header at cur+7+core+sig starts a valid flag=0 header w/ sane fields
        found=None
        for sig in range(0,64):
            no=cur+7+core+sig
            if no+7>len(strip): break
            nf,nd,ng,ns=parse_hdr(no)
            if nf==0 and nd<20000 and ng<20000 and ns<20000 and (nd+ng+ns)>0:
                found=sig; break
        off=cur-base
        print(f" lb{lb} @off{off} flag={flag} data={data} gcli={gcli} sign={sign} core={core} -> sig={found} (f20=11) region={7+core+(found or 0)}")
        if found is None: print("   no boundary found"); break
        cur=cur+7+core+found
    print(f" end cursor off={cur-base} (total={total})")
for p in [r"D:\_repos\_ref_libraw_he\oracle\strip_8070.bin"]:
    analyze(p)
