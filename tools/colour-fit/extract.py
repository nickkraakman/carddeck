import json, numpy as np
from PIL import Image
from register import SH
from sample import rects
reg=json.load(open('reg.json'))
def hexrgb(h): return [int(h[i:i+2],16) for i in (1,3,5)]
rows=[]
for name,f in SH.items():
    im=np.array(Image.open(f'{f}.png').convert('RGB')).astype(float)
    for side in ('front','back'):
        r=reg[name][side]; s=r['scale']; ox=r['x']-16; oy=r['y']-16
        # add white bg patch + black text patch
        items=[]
        for (x,y,w,h,fill,rot,sx,sy) in rects(side):
            if w!=100.394 or fill is None: continue
            if x>1013 or y>642: continue
            if y<100: y0,y1=175,225
            else: y0,y1=450,620
            x0=x+25; x1=x+75
            if x0>1013-10: continue
            items.append((fill,x0,x1,y0,y1))
        items.append(('#ffffff',440,700,250,290)) # white bg between rows? 
        for fill,x0,x1,y0,y1 in items:
            xa,xb=int(ox+x0*s),int(ox+x1*s); ya,yb=int(oy+y0*s),int(oy+y1*s)
            patch=im[ya:yb,xa:xb].reshape(-1,3)
            med=np.median(patch,axis=0)
            rows.append(dict(setting=name,side=side,fill=fill,src=hexrgb(fill),out=[float(v) for v in med],std=float(patch.std(axis=0).mean())))
json.dump(rows,open('swatches.json','w'))
for name in SH:
    rr=[r for r in rows if r['setting']==name]
    print(name, 'max std',max(r['std'] for r in rr))
    for r in rr[:3]+rr[-2:]: print('  ',r['side'],r['fill'],r['src'],r['out'])
