from PIL import Image,ImageDraw
from pathlib import Path
n=1024
im=Image.new('RGBA',(n,n),(0,0,0,0));mask=Image.new('L',(n,n));d=ImageDraw.Draw(mask);d.rounded_rectangle((28,28,996,996),radius=232,fill=255)
bg=Image.new('RGBA',(n,n));px=bg.load()
for y in range(n):
 for x in range(n):
  t=(x+y)/(2*n);px[x,y]=(int(13+9*t),int(40+22*t),int(49+24*t),255)
im.paste(bg,(0,0),mask);d=ImageDraw.Draw(im)
d.rounded_rectangle((211,185,750,856),radius=98,fill='#36716e')
d.rounded_rectangle((272,214,811,874),radius=91,fill='#eaf7ef')
d.rounded_rectangle((418,157,667,252),radius=43,fill='#a3daca')
d.rounded_rectangle((443,181,642,221),radius=18,fill='#1d4e54')
# Two graceful cubic sides form the leaf.
def bez(a,b,c,e):
 return [tuple((1-t)**3*a[k]+3*(1-t)**2*t*b[k]+3*(1-t)*t*t*c[k]+t**3*e[k] for k in (0,1)) for t in [i/80 for i in range(81)]]
leaf=bez((361,648),(312,423),(518,374),(712,347))+bez((712,347),(707,561),(558,739),(361,648))
d.polygon(leaf,fill='#23a68f');d.line(bez((342,711),(398,601),(513,491),(651,410)),fill='#174b53',width=22)
d.rounded_rectangle((452,765,681,787),radius=11,fill='#b8d5cd')
p=Path('assets');im.resize((512,512),Image.Resampling.LANCZOS).save(p/'pasteleaf-icon.png');im.save(p/'pasteleaf.ico',sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])
