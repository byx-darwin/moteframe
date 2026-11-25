// Mechanically transcribe approved static contracts; no rendering or model implementation.
import {spawnSync} from 'node:child_process';import fs from 'node:fs';import vm from 'node:vm';
const root=new URL('../../../designs/wattch5-retro-parity/',import.meta.url),out=new URL('../src/',import.meta.url),c=vm.createContext({});
for(const n of ['contracts.js','font.js','font-zh.js'])vm.runInContext(fs.readFileSync(new URL(n,root),'utf8'),c);
const C=c.ParityContract,pages=[...C.registry.map(x=>x[1]),...C.notices.map(x=>x[1]),...C.extras.map(x=>x[0])];
function write(name,source){if(process.argv.includes('--check')){const formatted=spawnSync('rustfmt',['--edition','2024','--emit','stdout'],{input:source,encoding:'utf8'});if(formatted.status!==0||formatted.stdout!==fs.readFileSync(new URL(name,out),'utf8'))throw Error('Generated source drift:'+name);}else{fs.writeFileSync(new URL(name,out),source);}}
const upper=s=>s[0].toUpperCase()+s.slice(1);
let s='//! Generated from the approved V2 contract; preserve registry order.\nuse crate::UiError;\n/// Finite presentation scenes, including registered source stubs.\n#[derive(Clone,Copy,Debug,PartialEq,Eq)]\n#[repr(u8)]\npub enum Screen {\n';
for(const p of pages)s+=`/// ${p} scene.\n${upper(p)},\n`;
s+='}\nimpl Screen {\n/// All62 scenes in source registry/notice/extra order.\npub const ALL:[Self;62]=['+pages.map(x=>'Self::'+upper(x)).join(',')+'];\n/// Stable host/reference identifier.\n#[must_use]\npub const fn id(self)->&\'static str {match self {\n';for(const p of pages)s+=`Self::${upper(p)}=>${JSON.stringify(p)},\n`;
s+='}}\n/// Parses a bounded scene identifier.\n/// # Errors\n/// Unknown identifiers are rejected.\npub fn from_id(s:&str)->Result<Self,UiError>{Self::ALL.into_iter().find(|p|p.id()==s).ok_or(UiError::UnknownScreen)}\n}\n';
write('screen.rs',s);
s='//! Fixed bilingual copy from contracts.js.\nuse crate::{Locale,Screen};\n/// Finite identifiers for approved bilingual copy.\n#[derive(Clone,Copy,Debug)]\n#[repr(u8)]\npub enum Label {\n'+Object.keys(C.labels).map(k=>`/// ${k}.\n${upper(k)},`).join('\n')+'\n}\n';
s+='static COPY:[[&str;2];147]='+JSON.stringify(Object.values(C.labels))+';\nimpl Label {\n/// Fixed copy for an implemented locale.\n#[must_use]\npub const fn text(self,locale:Locale)->&\'static str {COPY[self as usize][match locale {Locale::Zh=>1,Locale::En=>0}]}\n}\n';
const titles={};for(const p of pages){const key=p==='dial'?'boot':p;(titles[key]??=[]).push('Screen::'+upper(p));}
s+='pub(crate) const fn title(s:Screen)->Label {match s {\n'+Object.entries(titles).map(([p,names])=>names.join('|')+'=>Label::'+upper(p)+',').join('\n')+'\n}}\n';write('labels.rs',s);
const ascii=Object.entries(c.RetroFont),zh=Object.entries(c.RetroFontZh).sort((a,b)=>a[0].codePointAt(0)-b[0].codePointAt(0));
s='//! Wattch Retro CN Subset, derived OFL-1.1; see design FONT-NOTICE.md and licenses/.\n';
s+=`pub(crate) static ASCII:[u8;47]=${JSON.stringify(ascii.map(([k])=>k.charCodeAt(0)))};\npub(crate) static ASCII_ROWS:[[u8;7];47]=${JSON.stringify(ascii.map(([,v])=>v))};\n`;
s+=`pub(crate) static ZH_INDEX:[u16;242]=${JSON.stringify(zh.map(([k])=>k.charCodeAt(0)))};\npub(crate) static ZH_ROWS:[[u16;12];242]=${JSON.stringify(zh.map(([,v])=>v))};\n`;
write('font_data.rs',s);console.log('Generated62 scenes,',Object.keys(C.labels).length,'labels,47ASCII/242Chinese glyphs');
