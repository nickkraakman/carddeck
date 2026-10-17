import numpy as np, json
from PIL import Image
from scipy.signal import fftconvolve
from scipy.ndimage import sobel, gaussian_filter
SH={'none':'01M55T8ZN62BC57QA36X3NB38B','gamma1':'01M55T8ZP7WRCSZH247WMWKVC3','gamma2':'01M55T8ZN7N386JSX5684YAVN3',
'absolute':'01M55T8ZNGN7XFX8CB3E4VTCJF','relative':'01M55T8ZPMC68KH3CDGAWQSDEK','perceptual':'01M55T8ZR3SC0MES8NTVJ19AHR','saturation':'01M55T91EXKR1V5XCFZA1H6RZJ'}
def edges(a):
    g=a.mean(axis=2)
    g=gaussian_filter(g,1.0)
    return np.hypot(sobel(g,0),sobel(g,1))
def reg(shot, ref, y0frac, y1frac, scales):
    # shot: preview-region image (right half), ref: render rgb
    E=edges(shot.astype(float))
    best=None
    for s in scales:
        r=np.array(Image.fromarray(ref).resize((round(ref.shape[1]*s),round(ref.shape[0]*s)),Image.LANCZOS)).astype(float)
        T=edges(r)[8:-8,8:-8]; 
        T=T-T.mean()
        # normalized xcorr approx
        c=fftconvolve(E,T[::-1,::-1],mode='valid')
        # local energy normalisation
        e2=fftconvolve(E**2,np.ones_like(T),mode='valid'); 
        n=c/np.sqrt(e2+1e-6)
        i=np.unravel_index(np.argmax(n),n.shape)
        if best is None or n[i]>best[0]: best=(n[i],s,i[1]-0+0,i[0],)
    return best
if __name__=='__main__':
    out={}
    for k,f in SH.items():
        im=np.array(Image.open(f'/workspace/carddeck-fit/{f}.png').convert('RGB'))
        h,w,_=im.shape
        out[k]={}
        for side,rf in (('front','soft-summer-front'),('back','soft-summer-back')):
            ref=np.array(Image.open(f'/workspace/carddeck-fit/render/{rf}.png').convert('RGB'))
            x0=int(w*0.44)
            region=im[:, x0:]
            b=reg(region,ref,0,1,np.arange(0.80,0.90,0.004))
            # refine
            s0=b[1]; b=reg(region,ref,0,1,np.arange(s0-0.004,s0+0.0041,0.001))
            out[k][side]=dict(score=float(b[0]),scale=float(b[1]),x=int(b[2]+x0+8),y=int(b[3]+8))
            print(k,side,out[k][side],flush=True)
    json.dump(out,open('/workspace/carddeck-fit/reg.json','w'),indent=1)
