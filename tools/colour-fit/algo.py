import numpy as np
from lut import load
from display import s2l,l2s
a=load(); G=a[12:12+65**3*12].reshape(65,65,65,3,4).astype(np.int32)
INTENT={'perceptual':0,'saturation':1,'relative':2,'absolute':3}
def helix(rgb,intent):
    """rgb: (...,3) uint8-ish ints; returns (...,3) rgb ints"""
    rgb=np.asarray(rgb).astype(np.int32)
    lo=rgb//4; off=rgb-lo*4; up=lo+1
    base=G[lo[...,0],lo[...,1],lo[...,2],:,intent]
    hi=G[up[...,0],up[...,1],up[...,2],:,intent]
    adj=off*(hi-base)/4.0
    adj=np.where(adj<0,np.ceil(adj-0.5),np.floor(adj+0.5)).astype(np.int32)
    cmy=(base+adj)&0xff
    out=255-cmy
    black=np.all(rgb==0,axis=-1)
    out[black]=0
    return out
if __name__=='__main__':
    for p in ([255,255,255],[0,0,0],[1,1,1],[128,128,128],[74,74,74]):
        print(p,[helix(np.array(p),i).tolist() for i in range(4)])
