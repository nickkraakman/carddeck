import re, numpy as np, os
SRC='/workspace/magicard/linux_driver/magicard_ltd-linux_driver-1.4.0/src/colour-profiles/magir2x.c'
def load():
    if os.path.exists('magir2x.npy'): return np.load('magir2x.npy')
    t=open(SRC).read()
    body=t[t.index('{',t.index('MAGIR2X')) +1: t.rindex('}')]
    a=np.array([int(x) for x in re.findall(r'\d+',body)],dtype=np.uint8)
    np.save('magir2x.npy',a); return a
if __name__=='__main__':
    a=load(); print(a.size, (a.size-12)/12, 65**3*12+12)
