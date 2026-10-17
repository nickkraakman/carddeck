import json, numpy as np
from PIL import Image
from register import SH
reg=json.load(open('reg.json'))
card=json.load(open('/workspace/magicard/samples/soft-summer.card'))
def rects(side):
    out=[]
    for e in card[side]['elements']:
        if e.get('className')=='Rect' and e.get('visible',True):
            w,h=float(e['width']),float(e['height'])
            if w<150 and h<300 and w>40 and h>50 or True:
                out.append((e['x'],e['y'],w,h,e.get('fill'),e.get('rotation',0),e.get('scaleX',1),e.get('scaleY',1)))
    return out
if __name__=='__main__':
    for s in ('front','back'):
        for r in rects(s): print(s,r)
