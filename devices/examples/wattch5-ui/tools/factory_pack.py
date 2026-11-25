"""Read original C layouts/BMPs without modifying them; emit a private lossless skin.

No vendor images are embedded in source. Every original crop is range checked.
The original C's empty/stub windows remain identified in coverage.json.
"""
import argparse, ast, hashlib, json, re, struct
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont
from prepare import rgb565

COUNT=2336
START=32+COUNT*4
FONT=Path('/System/Library/Fonts/STHeiti Medium.ttc')
ZH_MENUS={
 'tools':['倒计时','秒表','天气','设置'],
 'settings':['设备信息','更换表盘','亮度调节','通知文字','语言切换','清除数据','重启设备','关机'],
 'sportList':['骑行','登山','羽毛球','跑步']}

def coordinate(text):
    def value(n):
        if isinstance(n,ast.Constant) and type(n.value) is int:return n.value
        if isinstance(n,ast.BinOp) and isinstance(n.op,(ast.Add,ast.Sub,ast.Mult)):
            a,b=value(n.left),value(n.right)
            return a+b if isinstance(n.op,ast.Add) else a-b if isinstance(n.op,ast.Sub) else a*b
        raise ValueError('unsupported source coordinate: '+text)
    try:r=value(ast.parse(text.strip(),mode='eval').body)
    except (SyntaxError,TypeError) as e:raise ValueError('invalid coordinate') from e
    if not -128<=r<=4096:raise ValueError('coordinate exceeds source bounds')
    return r

def pack(images):
    directory=[0]*COUNT;maps=bytearray();rows=bytearray();ids={};scene_ids={}
    for key,image in sorted(images.items()):
        if not 0<=key<COUNT or image.size!=(128,128):raise ValueError('invalid image geometry/key')
        raw=rgb565(image);mapping=bytearray()
        for y in range(128):
            row=raw[y*256:(y+1)*256]
            if row not in ids:
                ids[row]=len(rows)//256;rows.extend(row)
            mapping.extend(struct.pack('<H',ids[row]))
        mapping=bytes(mapping)
        if mapping not in scene_ids:
            scene_ids[mapping]=START+len(maps);maps.extend(mapping)
        directory[key]=scene_ids[mapping]
    end=START+len(maps)+len(rows)
    if end>0x200000:raise ValueError('factory skin exceeds immutable partition')
    return struct.pack('<8s6I',b'W5UIF001',1,COUNT,START,START+len(maps),end,len(scene_ids))+struct.pack('<'+str(COUNT)+'I',*directory)+maps+rows

def unpack_row(raw,key,y):
    if len(raw)<START or not 0<=key<COUNT or not 0<=y<128:raise ValueError('invalid row')
    magic,v,n,start,rows,end,scenes=struct.unpack_from('<8s6I',raw)
    if (magic,v,n,start)!=(b'W5UIF001',1,COUNT,START) or end!=len(raw):raise ValueError('invalid header')
    at=struct.unpack_from('<I',raw,32+key*4)[0]
    if at<START or (at-START)%256 or at+256>rows:raise ValueError('invalid map')
    row=rows+struct.unpack_from('<H',raw,at+y*2)[0]*256
    if row+256>end:raise ValueError('invalid pixel row')
    return bytes(raw[row:row+256])

class Source:
    def __init__(self,root):
        self.resources=root/'resources';self.code=root/'FullResources_改复位引脚/Sources/user'
        self.paths={};self.used={};self.layouts={};self.addresses={};self.gaps=set();self.frame_gaps=set()
        header=(self.resources/'flashresource.h').read_text()
        bases={k:int(v,16) for k,v in re.findall(r'#define\s+(MODULE_BAND_\w+_BASE)\s+(0x[0-9a-fA-F]+)',header)}
        for line in header.splitlines():
            m=re.match(r'#define\s+(UI_\S+)\s+.*?//.*?\\ui_8\\(.+)',line)
            if m:self.paths[m[1]]=m[2].replace('\\','/')
            m=re.match(r'#define\s+(UI_\S+)\s+\((\w+)\+(0x[0-9a-fA-F]+)\)',line)
            if m:self.addresses[m[1]]=bases[m[2]]+int(m[3],16)
    def sprite(self,macro,w,h,frame=0):
        path=self.resources/'ui_8'/self.paths[macro]
        if not path.exists():
            self.gaps.add(macro)
            self.frame_gaps.add(macro)
            return None
        image=Image.open(path)
        with image:
            if image.width!=w or (frame+1)*h>image.height:raise ValueError(f'{macro}: crop exceeds actual source ({w},{h},{frame}) {image.size}')
            result=image.crop((0,frame*h,w,(frame+1)*h)).convert('RGB')
        self.used[str(path.resolve())]=hashlib.sha256(path.read_bytes()).hexdigest()
        return result
    def table(self,file,name):
        paths=list(self.code.rglob(file))
        if len(paths)!=1:raise ValueError('ambiguous original layout '+file)
        p=paths[0];text=p.read_text(errors='replace')
        text=re.sub(r'/\*.*?\*/','',text,flags=re.S);text=re.sub(r'//[^\n]*','',text)
        m=re.search(r'const\s+gui_menu_t\s+'+name+r'\s*=\s*',text)
        if not m:raise ValueError('missing original layout '+name)
        table=text[m.end():text.index('\n};',m.end())]
        rows=[]
        for row in re.findall(r'\{([^{}]+)\}',table):
            cols=[s.strip() for s in row.split(',')]
            if len(cols)>=8 and cols[4] in self.paths:
                rows.append((*[coordinate(v) for v in cols[:4]],cols[4],coordinate(cols[6])))
        self.layouts[str(p.resolve())]=hashlib.sha256(p.read_bytes()).hexdigest()
        return rows

# Actual source menu/table, rather than the old retro screenshots.
TABLES={
 'boot':('UI_win_boot.c','menu_boot_logo'), 'activity':('UI_win_pedometer.c','menu_pedometer'),
 'sport':('UI_win_sport.c','menu_sport'), 'heart':('UI_win_hr.c','menu_heart_rate'),
 'pressure':('UI_win_bpressure.c','menu_bp_main'), 'sleep':('UI_win_sleep.c','menu_sleep'),
 'messages':('UI_win_message.c','menu_message'), 'music':('UI_win_music.c','menu_music'),
 'weather':('UI_win_weather.c','menu_weather'), 'brightness':('UI_win_set_light.c','menu_set_light'),
 'clear':('UI_win_set_clear.c','menu_set_clear_data'), 'power':('UI_win_set_power.c','menu_set_power'),
 'restart':('UI_win_set_reboot.c','menu_set_reboot'), 'language':('UI_win_set_language.c','menu_set_lan'),
 'textsize':('UI_win_set_font_size.c','menu_set_lan'), 'temperature':('UI_win_temperature.c','menu_temp'),
 'charge':('UI_win_charge.c','menu_charge'), 'ota':('UI_win_ota.c','menu_ota'),
 'quick':('UI_win_shortcut.c','menu_shortcut'), 'glucose':('UI_win_sugar.c','menu_sugar_main'),
 'hba1c':('UI_win_HbA1c.c','menu_sugar_main'), 'measureTips':('UI_win_sugar.c','menu_sugar_tips'),
 'measureConfirm':('UI_win_sugar.c','menu_sugar_choose'),
 'sportCountdown':('UI_win_sport.c','menu_sport_ready'), 'sportRun':('UI_win_sport.c','menu_sport_timer'),
 'sportControls':('UI_win_sport.c','menu_sport_pause'), 'unlock':('UI_win_sport.c','menu_sport_locked_unlock'),
 'face':('UI_win_set_view.c','menu_set_preview'),
}

def variants(page):
    return {'dial':6,'tools':6,'sportList':6,'measureTips':6,'face':8,'settings':9,'language':9,'brightness':5,'quick':3,'theme':3,'format':2,'textsize':2}.get(page,1)

def draw_source(source,page,locale,variant):
    image=Image.new('RGB',(128,128));draw=ImageDraw.Draw(image);language=locale
    def text(s,x,y,size=14):draw.text((x,y),s,font=ImageFont.truetype(str(FONT),size),fill='white')
    source.frame_gaps.clear()
    def paste(macro,x,y,w,h,frame=0):
        sprite=source.sprite(macro,w,h,frame)
        if sprite is not None:image.paste(sprite,(x,y))
    def missing(x,y,w,h):
        draw.rectangle((x,y,x+w-1,y+h-1),fill=0)
        draw.rectangle((x+2,y+h//2,x+w-3,y+h//2+1),fill='white')
    if page in ('tools','settings','sportList'):
        if page=='tools':
            labels=['UI_FUNC_LIST_DOWNCOUNT_80x168','UI_FUNC_LIST_STOPWATCHER_80x168','UI_FUNC_LIST_WEATHER_80x168','UI_FUNC_LIST_SETTINGS_80x168'];icon='UI_FUNC_LIST_ICON_36x180';offset=1
        elif page=='settings':
            labels=['UI_SET_DEVICE_INFORMATION_80x168','UI_SET_DIAL_REPLACEMENT_80x168','UI_SET_DIMMING_80x168','UI_SET_NOTIFICATION_TEXT_80x168','UI_SET_LANGUAGE_SWITCH_80x168','UI_SET_CLEAR_DATA_80x168','UI_SET_RESTART_THE_DEVICE_80x168','UI_SET_SHUT_DOWN_80x168'];icon='UI_SET_ICON_LIST_36x288';offset=0
        else:
            labels=['UI_SPORT_NAME_CYCLING_80x168','UI_SPORT_NAME_MOUNTAINEERING_80x168','UI_SPORT_NAME_BADMINTON_80x168','UI_SPORT_NAME_RUNNING_80x168'];icon='UI_SPORT_LIST_1_36x180';offset=0
        for row in range(3):
            i=variant//3*3+row
            if i<len(labels):
                paste(icon,4,6+40*row,36,36,i+offset)
                if locale==0:text(ZH_MENUS[page][i],46,[16,55,97][row])
                else:paste(labels[i],46,[16,55,97][row],80,21,0)
        return image,'original-menu'
    if page=='dial':
        tables=[('UI_dial_style_2.c','menu_dial_2'),('UI_dial_style_3.c','menu_dial_3'),('UI_dial_style_4.c','menu_dial_4')]
        rows=source.table(*tables[variant//2])
    elif page in TABLES:rows=source.table(*TABLES[page])
    else:rows=[]
    for index,(x,y,w,h,macro,enabled) in enumerate(rows):
        if not enabled:continue
        # Source confirmation descriptors use a44px hit rectangle but22px sprite.
        if macro in ('UI_YES_2_57x22','UI_NO_2_57x22'):h=22
        if '__0__9' in macro or '____0__9' in macro or macro.startswith(('UI_WEATHER_NUM_','UI_WEATHER_DATE_NUM_')) or macro in ('UI_WE_1_53x630','UI_TF__122_29x470','UI_TF__128_16x280','UI_DAIL_BU_TIME_1_14x260','UI_SPORT_NUM_13x170','UI_SPORT_NUM_SMALL_10x130'):
            missing(x,y,w,h);continue
        frame=0
        path=source.paths[macro]
        if path.startswith('语言/') and 'UI_LAN_SWITCH' not in macro:frame=language if '/中英/' in path or '/星期中英/' in path else 0
        if macro=='UI_LIGHTNESS_NUMBER_SMALL_10x55':frame=(variant+(4 if y<49 else 1))%5
        if macro=='UI_LIGHTNESS_NUMBER_13x70':frame=variant
        if macro=='UI_DIAL_PREVIEW_80x400':frame=variant//2
        if macro=='UI_LAN_SWITCH_52x120':
            if y==37:text('中文',38,34);continue
            frame=0
        if macro in ('UI_ARROW_RIGHT_9x11','UI_ARROW_LEFT_9x11'):y=39 if variant==0 else 66
        if page in ('glucose','hba1c') and macro.startswith('UI_SUGAR11'):frame=0 if locale==0 else 1
        if macro in ('UI_HR_BG_128x354','UI_BLOOD_PRESSURE_128x354'):frame=5
        if macro=='UI_WEATHER_ICON_48x336':frame=6
        if page=='quick' and macro in ('UI_HFT__3_14x54','UI_SUN_SHORT_46x112'):missing(x,y,w,h);continue
        if macro=='UI_SHORTCUT_ICON_LIST_1_40x240':frame={2:0,44:1,86:4}[x]
        if macro=='UI_SPORT_LIST_2_40x240':frame=[2,0,1,3][index]
        if page=='textsize' and 'FONT' in macro:frame=variant
        paste(macro,x,y,w,h,frame)
    if page=='oxygen':
        paste('UI_OXYGEN_TITLE_70x152',1,0,70,19,language)
        paste('UI_BLOOD_OXYGEN__1_14x18',13,25,14,18)
        paste('UI_BLOOD_OXYGEN_PECENT_11x10',84,29,11,10)
        paste('UI_BLOOD_OXYGEN_128x354',0,44,128,59,5)
        for x in [36,51,66]:missing(x,25,13,17)
    elif page=='findphone':paste('UI_FIND_A_PHONE_80x320',24,24,80,80)
    elif page in ('glucose','hba1c'):
        # Main is the source entry illustration. Actual imported values get a
        # separate result cell without turning an unavailable source into zero.
        for x in [37,52,74]:missing(x,37,13,17)
        text(('血糖' if page=='glucose' else '糖化血红蛋白') if locale==0 else page,2,2)
    elif page=='temperature' and 'UI_TEMPERATURE_TITLE_70x152' in source.gaps:text('体温' if locale==0 else 'TEMP',1,0)
    if page in ('heart','pressure','oxygen'):
        # The old bitmaps contain illustrative curves/bars, not caller history.
        # Retain an empty chart until a real history provider is connected.
        top=56 if page=='pressure' else 44
        draw.rectangle((0,top,127,top+58),fill=0)
        for y in range(top,top+59,15):draw.line((0,y,127,y),fill=(45,45,45))
    if page=='stopwatch':
        paste('UI_STOPWATCH_TITLE_70x152',0,0,70,19,language)
        paste('UI_START_1_57x22',68,106,57,22);paste('UI_RESTORE_57x22',4,106,57,22)
        return image,'original-direct-draw'
    if page=='about':
        for macro,x,y,w,h in [('UI_ABOUT_44x112',42,4,44,14),('UI_MODEL_46x104',13,24,46,13),('UI_FIRMWARE_68x120',13,58,68,15)]:paste(macro,x,y,w,h,0)
        return image,'original-direct-draw'
    if page in ('timer','timerCustom','timerRun'):
        paste('UI_COUNTDOWN_TITLE_70x152',0,0,70,19,language)
        if page=='timer':
            for macro,x,y in [('UI_MINUTE_1_46x36',12,25),('UI_MINUTE_3_46x36',70,25),('UI_MINUTE_5_46x36',12,65),('UI_MINUTE_10_46x36',70,65)]:paste(macro,x,y,46,36)
            paste('UI_CUSTOMIZE_1_106x192',11,104,106,24,0)
        elif page=='timerCustom':paste('UI_CONFIRM_1_128x24',0,104,128,24)
        else:
            paste('UI_DOWNCONT_START_PAUSE_STOP_36x108',0,92,36,36,0);paste('UI_DOWNCONT_START_PAUSE_STOP_36x108',92,92,36,36,1)
        return image,'original-direct-draw'
    notices={'noticeAlarm':('UI_ALARM_30x30',49,20,30,30),'alarm':('UI_ALARM_30x30',49,20,30,30),'camera':('UI_TAKEP_HOTO_70x71',28,28,70,71),'noticeFind':('UI_FIND_A_BRACELET_80x320',24,24,80,80),'noticeMove':('UI_SEDENTARY_48x62',40,33,48,62),'noticeHeart':('UI_HR_ALERT_60x64',34,20,60,64),'noticeBattery':('UI_LOW_POWER__1_32x52',48,20,32,52)}
    if page in notices:
        macro,x,y,w,h=notices[page];paste(macro,x,y,w,h)
        return image,'original-notification'
    if not rows and page not in ('oxygen','findphone'):
        # Source stubs/accessory scenes: explicit plain text, never a retro image.
        draw.text((4,7),page.upper(),fill='white')
        draw.text((43,51),'--',fill='white')
        if page in ('timer','timerCustom','timerRun','stopwatch','sportEnd','reset'):
            draw.rectangle((0,103,127,126),outline='white');draw.text((38,109),'OK',fill='white')
        return image,'source-stub-or-accessory'
    return image,'original-layout-with-missing-resources' if source.frame_gaps else 'original-layout'

def build(root,output,registry):
    source=Source(root);ids=re.findall(r'Self::\w+ => "(\w+)"',registry.read_text())
    if len(ids)!=62:raise ValueError('source scene registry changed')
    images={};coverage=[];output.mkdir(parents=True,exist_ok=True)
    for page_no,page in enumerate(ids):
        for locale in range(2):
            for variant in range(variants(page)):
                image,kind=draw_source(source,page,locale,variant)
                key=(page_no*2+locale)*18+variant;images[key]=image
                if variant==0:
                    image.save(output/(f'{page}-'+('zh.png' if locale==0 else 'en.png')))
                    coverage.append({'screen':page,'locale':['zh','en'][locale],'source':kind})
    for i in range(10):
        image=Image.new('RGB',(128,128));image.paste(source.sprite('UI_JB__0__9_13x170',13,17,i),(0,0));images[2232+i]=image
    packed=pack(images);(output/'factory-ui.bin').write_bytes(packed)
    # Validate every actual generated pixel against the original crop composition.
    for key,image in images.items():
        actual=b''.join(unpack_row(packed,key,y) for y in range(128))
        if actual!=rgb565(image):raise ValueError('pixel mismatch '+str(key))
    sheet=Image.new('RGB',(6*260,6*286),(27,29,33));canvas=ImageDraw.Draw(sheet)
    selected=['dial','activity','heart','pressure','oxygen','sleep','glucose','hba1c','temperature','sport','tools','settings','brightness','language','weather','music','messages','quick','clear','power','restart','face','charge','ota','sportCountdown','sportRun','sportControls','unlock','findphone','boot','stopwatch','timer','about','alarm','textsize','measureConfirm']
    for i,page in enumerate(selected):
        image=images[(ids.index(page)*2)*18]
        x=(i%6)*260;y=(i//6)*286
        sheet.paste(image.resize((256,256),Image.Resampling.NEAREST),(x,y+23));canvas.text((x+4,y+5),page,fill='white')
    sheet.save(output/'overview.png')
    report={'format':'W5UIF001','bytes':len(packed),'sha256':hashlib.sha256(packed).hexdigest(),'vendor_resources_included':True,'private':True,'generated_scenes':len(images)-10,'full_scene_pixels_verified':len(images)*16384,'coverage':coverage,'missing_original_resources':sorted(source.gaps),'supplemental_zh_labels':{'font':str(FONT),'sha256':hashlib.sha256(FONT.read_bytes()).hexdigest(),'reason':'original menu language strips omit Chinese; generated labels are not vendor pixels'},'resources':source.used,'layouts':source.layouts,'hardware_execution':False}
    (output/'coverage.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k not in ('coverage','resources','layouts')}))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--source',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--registry',type=Path,default=Path('examples/wattch5-parity-ui/src/screen.rs'))
    args=p.parse_args();build(args.source,args.output,args.registry)
