"""Read-only original BMP preparation and independent C-layout pixel references."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
from PIL import Image

# Bundle order is AssetId in src/assets.rs. Use source language frame0 (mixed labels).
ASSETS = [
 ('UI_WE_1_53x630',53,63,10),('UI_FUNC_LIST_ICON_36x180',36,36,5),
 ('UI_FUNC_LIST_DOWNCOUNT_80x168',80,21,1),('UI_FUNC_LIST_STOPWATCHER_80x168',80,21,1),
 ('UI_FUNC_LIST_WEATHER_80x168',80,21,1),('UI_FUNC_LIST_SETTINGS_80x168',80,21,1),
 ('UI_SET_ICON_LIST_36x288',36,36,8),('UI_SET_DEVICE_INFORMATION_80x168',80,21,1),
 ('UI_SET_DIAL_REPLACEMENT_80x168',80,21,1),('UI_SET_DIMMING_80x168',80,21,1),
 ('UI_SET_NOTIFICATION_TEXT_80x168',80,21,1),('UI_SET_LANGUAGE_SWITCH_80x168',80,21,1),
 ('UI_SET_CLEAR_DATA_80x168',80,21,1),('UI_SET_RESTART_THE_DEVICE_80x168',80,21,1),
 ('UI_SET_SHUT_DOWN_80x168',80,21,1),('UI_DIMMING_TITLE_70x152',70,19,1),
 ('UI_HJ__0__9_10x130',10,13,10),('UI_HJ__1_4x13',4,13,1),
 ('UI_LIGHTNESS_BG_128x24',128,24,1),('UI_LIGHTNESS_NUMBER_SMALL_10x55',10,11,5),
 ('UI_LIGHTNESS_NUMBER_13x70',13,14,5),('UI_CONFIRM_1_128x24',128,24,1)]

def rgb565(image):
    data=bytearray()
    for red,green,blue in image.convert('RGB').getdata():
        data.extend((((red>>3)<<11)|((green>>2)<<5)|(blue>>3)).to_bytes(2,'big'))
    return bytes(data)

def strip(image,width,height,count):
    if width<=0 or height<=0 or count<=0 or image.width!=width or image.height%height or image.height<height*count:
        raise ValueError('original strip geometry differs; never resize or pad')
    return image.crop((0,0,width,height*count)).convert('RGB')

def pack(images):
    if len(images)!=22:raise ValueError('expected22 assets')
    offset=274;records=bytearray();data=bytearray()
    for image in images:
        pixels=rgb565(image)
        records.extend(struct.pack('>HHII',image.width,image.height,offset,len(pixels)))
        data.extend(pixels);offset+=len(pixels)
    return b'W5UI\x01\0\0\0'+struct.pack('>H',22)+records+data

def widgets(path):
    text=path.read_text(errors='strict');start=text.index('const gui_menu_t');table=text[start:text.index('\n};',start)]
    rows=re.findall(r'\{\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*(UI_\w+)',table)
    return [(int(x),int(y),int(w),int(h),macro) for x,y,w,h,macro in rows]

def prepare(root,output):
    resources=root/'resources';code=root/'FullResources_改复位引脚/Sources/user'
    header=resources/'flashresource.h';paths={}
    for line in header.read_text().splitlines():
        match=re.match(r'#define\s+(UI_\S+)\s+.*?//.*?\\ui_8\\(.+)',line)
        if match:paths[match[1]]=match[2].replace('\\','/')
    output.mkdir(parents=True,exist_ok=True);images=[];provenance=[]
    for macro,w,h,count in ASSETS:
        path=resources/'ui_8'/paths[macro]
        with Image.open(path) as image:selected=strip(image,w,h,count)
        images.append(selected)
        provenance.append({'macro':macro,'source':str(path.resolve()),'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'selected_size':selected.size})
    bundle=pack(images);(output/'bundle.bin').write_bytes(bundle)
    sources=[code/'dial/ui_dial_style_2.c',code/'win/UI_win_func_list.c',code/'win/UI_win_set.c',code/'win/UI_win_set_light.c']
    layouts=[widgets(p) for p in sources]
    if list(map(len,layouts))!=[4,6,6,11]:raise ValueError('source widget table count changed')
    def composite(name,items):
        reference=Image.new('RGB',(128,128),'black')
        for asset,frame,geometry in items:
            x,y,w,h,_=geometry
            if images[asset].width!=w:raise ValueError('source layout and original bitmap disagree')
            reference.paste(images[asset].crop((0,frame*h,w,(frame+1)*h)),(x,y))
        (output/(name+'.reference.rgb565')).write_bytes(rgb565(reference))
        reference.save(output/(name+'.reference.png'))
    composite('dial',[(0,n,r) for n,r in zip([1,0,5,0],layouts[0])])
    for page in range(2):
        ids=range(page*3,min(page*3+3,4));items=[(1,i+1,layouts[1][k]) for k,i in enumerate(ids)]
        items += [(2+i,0,layouts[1][k+3]) for k,i in enumerate(ids)]
        composite('menu'+str(page),items)
    for page in range(3):
        ids=range(page*3,min(page*3+3,8));items=[(6,i,layouts[2][k]) for k,i in enumerate(ids)]
        items += [(7+i,0,layouts[2][k+3]) for k,i in enumerate(ids)]
        composite('settings'+str(page),items)
    for level in range(5):
        choices=[(15,0),(16,1),(16,0),(17,0),(16,5),(16,0),(18,0),(19,(level+4)%5),(20,level),(19,(level+1)%5),(21,0)]
        composite('brightness'+str(level),[(asset,frame,rect) for (asset,frame),rect in zip(choices,layouts[3])])
    report={'product':'wattch 5','scope':'Source language frame0 first-batch reference; demo time10:50, brightness0..4','bundle_sha256':hashlib.sha256(bundle).hexdigest(),'bundle_bytes':len(bundle),'assets':provenance,'layout_sources':{str(p.resolve()):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},'header_sha256':hashlib.sha256(header.read_bytes()).hexdigest()}
    (output/'provenance.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print('Prepared',len(images),'assets,',len(bundle),'bytes;11independent C-layout references')

def verify(output):
    results=[];sheet=Image.new('RGB',(512*4,560*3),(30,30,30))
    for index,path in enumerate(sorted(output.glob('*.reference.rgb565'))):
        name=path.name.removesuffix('.reference.rgb565');actual=(output/(name+'.actual.rgb565')).read_bytes();expected=path.read_bytes()
        if len(actual)!=32768 or actual!=expected:raise ValueError(name+' differs from original C-layout reference')
        pixels=[]
        for offset in range(0,len(actual),2):
            p=int.from_bytes(actual[offset:offset+2],'big');pixels.append(((p>>11)*255//31,((p>>5)&63)*255//63,(p&31)*255//31))
        image=Image.new('RGB',(128,128));image.putdata(pixels);image.save(output/(name+'.actual.png'))
        sheet.paste(image.resize((512,512),Image.Resampling.NEAREST),((index%4)*512,(index//4)*560))
        results.append({'name':name,'pixels':16384,'mismatches':0,'actual_sha256':hashlib.sha256(actual).hexdigest()})
    if len(results)!=11:raise ValueError('missing reference states')
    sheet.save(output/'contact-sheet.png');(output/'comparison.json').write_text(json.dumps(results,indent=2)+'\n')
    print('PASS',len(results)*16384,'pixels, zero mismatches')

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--source',type=Path);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--verify',action='store_true');args=parser.parse_args()
    if args.verify:verify(args.output)
    elif args.source:prepare(args.source,args.output)
    else:parser.error('--source required to prepare')
