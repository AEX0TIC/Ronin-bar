# Draws the Ronin Bar logo (ensō + katana): python3 logo.py <out dir> <preview.png>. Needs Pillow; not used at build time.
from PIL import Image, ImageDraw
import math, sys
S=1024
def mark(bg=True):
    im=Image.new('RGBA',(S,S),(0,0,0,0)); d=ImageDraw.Draw(im)
    if bg: d.rounded_rectangle([24,24,S-24,S-24],radius=220,fill=(12,11,14,255))
    cx,cy,R=S/2,S/2,300
    start,end=78,78+318   # counter-clockwise from upper right round to upper right: gap at ~36..78 deg
    n=1200
    for i in range(n+1):
        t=i/n; a=math.radians(start+(end-start)*t)
        w=24+76*((1-t)**0.55)*min(1,t*10+0.35)    # loaded brush, thinning to a dry tail
        r=R+6*math.sin(t*7)
        x,y=cx+r*math.cos(a),cy-r*math.sin(a)
        d.ellipse([x-w/2,y-w/2,x+w/2,y+w/2],fill=(209,38,59,255))
    ang=math.radians(54); ux,uy=math.cos(ang),-math.sin(ang); nx,ny=-uy,ux
    def P(s,o): return (cx+ux*s+nx*o, cy+uy*s+ny*o)
    W=17
    d.polygon([P(-170,-W),P(350,-W*0.7),P(430,4),P(350,W*0.7),P(-170,W)],fill=(237,232,223,255))
    d.polygon([P(-170,W),P(350,W*0.7),P(430,4),P(350,5),P(-170,5)],fill=(160,155,148,255))
    d.rounded_rectangle([0,0,1,1],0)
    d.polygon([P(-186,-46),P(-166,-46),P(-166,46),P(-186,46)],fill=(237,232,223,255))
    d.polygon([P(-410,-19),P(-186,-21),P(-186,21),P(-410,19)],fill=(209,38,59,255))
    for k in range(-392,-196,38): d.polygon([P(k,-21),P(k+15,-21),P(k+15,21),P(k,21)],fill=(12,11,14,255))
    return im
out=sys.argv[1]
mark(False).resize((256,256),Image.LANCZOS).save(out+'/ronin-mark.png')
m=mark(True); m.resize((256,256),Image.LANCZOS).save(out+'/ronin.png')
m.save(out+'/ronin.ico',sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])
a=Image.open(out+'/ronin.png'); b=Image.new('RGBA',(600,300),(240,240,240,255)); b.paste(a,(10,22),a)
for i,s in enumerate([16,24,32,48]):
    r=a.resize((s,s),Image.LANCZOS); b.paste(r,(300+i*70,140),r)
b.save(sys.argv[2])
