import json, numpy as np
from display import s2l,l2s,fit_matrix
from algo import helix,INTENT
rows=json.load(open('swatches.json'))
M,_,_=fit_matrix()
S2P3=np.array([[0.8225,0.1774,0],[0.0332,0.9669,0],[0.0171,0.0724,0.9108]])
Minv=np.linalg.inv(M)
def p3_to_srgb(v,clip=True): return l2s(s2l(v)@Minv.T)
def srgb_to_p3(v): return l2s(s2l(v)@M.T)
def report(name,pred_fn,verbose=False):
    rr=[r for r in rows if r['setting']==name and r['std']<5]
    src=np.array([r['src'] for r in rr]); out=np.array([r['out'] for r in rr])
    truth=np.clip(np.round(p3_to_srgb(out)),0,255)
    pred=pred_fn(src)
    e=np.abs(truth-pred)
    d=np.sqrt(((truth-pred)**2).sum(1))
    if verbose:
        for r,t,p in zip(rr,truth,pred): print(r['side'],r['fill'],t,p)
    return dict(n=len(rr),mean=float(e.mean()),max=float(e.max()),meanE=float(d.mean()),maxE=float(d.max()))
if __name__=='__main__':
    print('none',report('none',lambda s:s))
    for k,i in INTENT.items():
        print(k,report(k,lambda s:helix(s,i)))
