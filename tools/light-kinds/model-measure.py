"""Measures how bright each model is against the floor below it.

    python -I model-measure.py <shots folder> <mask label> <label>...

The model is the magenta region nearest the middle of the mask label's
picture (KJK_MODEL_LIGHT=mask). The floor is a band 6 to 40 pixels below
the model, over its columns. It prints the model's mean luma over the
floor's for each pose and label. It needs Pillow.
"""
import sys, os
from PIL import Image
def luma(p): return 0.2126*p[0]+0.7152*p[1]+0.0722*p[2]
def region(mask):
    w,h=mask.size; px=mask.load()
    mg=lambda x,y:(lambda p:p[0]>200 and p[1]<60 and p[2]>200)(px[x,y])
    seen=set(); best=None
    for y in range(0,h,2):
        for x in range(0,w,2):
            if (x,y) in seen or not mg(x,y): continue
            st=[(x,y)]; comp=[]; seen.add((x,y))
            while st:
                a,b=st.pop(); comp.append((a,b))
                for na,nb in ((a+1,b),(a-1,b),(a,b+1),(a,b-1)):
                    if 0<=na<w and 0<=nb<h and (na,nb) not in seen and mg(na,nb):
                        seen.add((na,nb)); st.append((na,nb))
            if len(comp)<200: continue
            mx=sum(p[0] for p in comp)/len(comp); my=sum(p[1] for p in comp)/len(comp)
            d=(mx-w/2)**2+(my-h/2)**2
            if best is None or d<best[0]: best=(d,comp)
    if best is None: return None,None
    comp=best[1]; xs=[p[0] for p in comp]; low=max(p[1] for p in comp)
    floor=[(x,y) for y in range(low+6,min(h,low+41)) for x in range(min(xs),max(xs)+1) if not mg(x,y)]
    return comp,floor
def table(folder,mask_label,labels):
    out={}
    poses=sorted(f[:-4] for f in os.listdir(os.path.join(folder,mask_label)) if f.endswith('.png'))
    for pose in poses:
        comp,floor=region(Image.open(os.path.join(folder,mask_label,pose+'.png')).convert('RGB'))
        if not comp or not floor: out[pose]=None; continue
        out[pose]={}
        for label in labels:
            p=os.path.join(folder,label,pose+'.png')
            if not os.path.exists(p): continue
            px=Image.open(p).convert('RGB').load()
            o=sum(luma(px[q]) for q in comp)/len(comp); f=sum(luma(px[q]) for q in floor)/len(floor)
            out[pose][label]=(o,f,o/max(f,0.5))
    return out
if __name__=='__main__':
    folder,mask,labels=sys.argv[1],sys.argv[2],sys.argv[3:]
    t=table(folder,mask,labels)
    print('pose'.ljust(20)+''.join(l[:14].rjust(15) for l in labels))
    for pose,row in t.items():
        if row is None: print(pose.ljust(20)+'  no object'); continue
        print(pose.ljust(20)+''.join(('%.2f'%row[l][2]).rjust(15) if l in row else '-'.rjust(15) for l in labels))
