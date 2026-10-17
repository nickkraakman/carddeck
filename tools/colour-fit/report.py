import json, subprocess, numpy as np, os
from PIL import Image
from register import SH
from sample import rects
from evalfit import p3_to_srgb, M
from display import s2l,l2s
reg=json.load(open('reg.json'))
CLI='/workspace/carddeck/target/release/carddeck'
CARD='/workspace/magicard/samples/soft-summer.card'
NAMES={'none':'None','gamma1':'Gamma1','gamma2':'Gamma2','perceptual':'Perceptual','saturation':'Saturation','relative':'RelativeColorimetric','absolute':'AbsoluteColorimetric'}
def patches(side):
    out=[]
    for (x,y,w,h,fill,rot,sx,sy) in rects(side):
        if w!=100.394 or fill is None or x>1013 or y>642: continue
        if y<100: y0,y1=175,225
        else: y0,y1=450,620
        if x+25>1003: continue
        out.append((fill,int(x+25),int(x+75),y0,y1))
    out.append(('#ffffff',440,700,250,290))
    return out
res={}
for k,f in SH.items():
    d=f'/workspace/carddeck-fit/out/{k}'; os.makedirs(d,exist_ok=True)
    subprocess.run([CLI,'render',CARD,d,'--correction',NAMES[k]],check=True,capture_output=True)
    shot=np.array(Image.open(f'{f}.png').convert('RGB')).astype(float)
    errs=[];per=[]
    full=[]
    for side in ('front','back'):
        ours=np.array(Image.open(f'{d}/soft-summer-{side}.png').convert('RGBA'))
        r=reg[k][side]; s=r['scale']; ox=r['x']-16; oy=r['y']-16
        for fill,x0,x1,y0,y1 in patches(side):
            if side=='back' and y0>400 and x0<160: continue   # '34%' zoom widget overlaps this swatch in the screenshots
            if side=='back' and fill in ('#8a7197','#7d7aa5'): continue
            xa,xb=int(ox+x0*s),int(ox+x1*s); ya,yb=int(oy+y0*s),int(oy+y1*s)
            truth=np.clip(np.round(p3_to_srgb(np.median(shot[ya:yb,xa:xb].reshape(-1,3),axis=0))),0,255)
            mine=ours[y0:y1,x0:x1,:3].reshape(-1,3).astype(float).mean(0)
            errs.append(np.abs(truth-mine)); per.append((side,fill,truth.tolist(),np.round(mine).tolist()))
        # full-card comparison on the card interior, our render downsampled to the screenshot scale
        W,H=1013,642
        o=Image.fromarray(ours).resize((round(W*s),round(H*s)),Image.BOX)
        oa=np.array(o).astype(float)
        h,w=oa.shape[:2]
        crop=shot[oy:oy+h,ox:ox+w]
        if crop.shape[:2]!=(h,w): continue
        sr=np.clip(p3_to_srgb(crop.reshape(-1,3)),0,255).reshape(h,w,3)
        full.append((oa[...,:3],sr,oa[...,3]))
    e=np.array(errs)
    res[k]=dict(swatches=len(e),mean=float(e.mean()),max=float(e.max()),rmse=float(np.sqrt((e**2).mean())),full=full,per=per)
json.dump({k:{kk:vv for kk,vv in v.items() if kk not in('full',)} for k,v in res.items()},open('report.json','w'))
# full-card: mask where our None render agrees with the None screenshot (excludes font-substituted text) and card is opaque
masks=[]
for i,(oa,sr,a) in enumerate(res['none']['full']):
    m=(np.abs(oa-sr).max(2)<12)&(a>250)
    # erode: ignore pixels near edges
    from scipy.ndimage import binary_erosion
    m=binary_erosion(m,iterations=3)
    masks.append(m)
print(f"{'setting':12s} swatches  swatch mean  max   | full-card (outside text) mean  p99  max")
for k in SH:
    fe=np.concatenate([np.abs(oa-sr)[m].ravel() for (oa,sr,a),m in zip(res[k]['full'],masks)])
    print(f"{k:12s} {res[k]['swatches']:5d}   {res[k]['mean']:8.2f} {res[k]['max']:6.1f}   | {fe.mean():8.2f} {np.percentile(fe,99):6.1f} {fe.max():6.1f}")
