import json, numpy as np
rows=json.load(open('swatches.json'))
def s2l(v):
    v=np.asarray(v,float)/255
    return np.where(v<=0.04045,v/12.92,((v+0.055)/1.055)**2.4)
def l2s(l):
    l=np.clip(l,0,1)
    return 255*np.where(l<=0.0031308,12.92*l,1.055*l**(1/2.4)-0.055)
def fit_matrix():
    rr=[r for r in rows if r['setting']=='none' and r['std']<5]
    X=np.array([s2l(r['src']) for r in rr]); Y=np.array([s2l(r['out']) for r in rr])
    M,res,_,_=np.linalg.lstsq(X,Y,rcond=None)
    return M.T,X,Y
if __name__=='__main__':
    M,X,Y=fit_matrix()
    print(np.round(M,4)); print('row sums',M.sum(1))
    P=l2s(X@M.T); rr=[r for r in rows if r['setting']=='none' and r['std']<5]
    err=np.abs(P-np.array([r['out'] for r in rr]))
    print('mean abs err',err.mean(),'max',err.max())
    # compare with display p3 matrix from sRGB
    S2P3=np.array([[0.8225,0.1774,0],[0.0332,0.9669,0],[0.0171,0.0724,0.9108]])
    P=l2s(X@S2P3.T); print('P3 err',np.abs(P-np.array([r['out'] for r in rr])).mean())
    S2A=np.linalg.inv(np.array([[0.5767,0.1856,0.1882],[0.2974,0.6273,0.0753],[0.0270,0.0707,0.9911]]))@np.array([[0.4124,0.3576,0.1805],[0.2126,0.7152,0.0722],[0.0193,0.1192,0.9505]])
    print('adobe',np.round(S2A,3)); P=l2s(X@S2A.T); print('adobe err',np.abs(P-np.array([r['out'] for r in rr])).mean())
