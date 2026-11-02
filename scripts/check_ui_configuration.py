#!/usr/bin/env python3
"""Compile real offline consumers for UI features and invalid ownership/API uses."""
from pathlib import Path
import itertools
import json
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/ui-config-implementation/consumers'
COMMON = '''#![no_std]
use moteframe_ui as ui;
use ui::graphics::pixelcolor::BinaryColor as C;
use moteframe_core::{Dimensions, Display, Rect};
struct Screen;
impl Display for Screen {
 type Color=C; type Error=();
 fn dimensions(&self)->Dimensions {Dimensions::new(2,2)}
 fn write(&mut self,_:Rect,_:&[C])->Result<(),()> {Ok(())}
}
fn settings()->ui::UiSettings<C> {ui::UiSettings::new(ui::Palette::new(C::Off,C::On,C::On,C::Off,C::On))}
'''


def effective(features):
    result = set(features)
    if result & {'damage', 'frame-pacing'}:
        result.add('refresh')
    return result


def validate_build(result, source, success, diagnostic=None):
    """Accept rejected consumers only for their expected structured errors.

    Warning text, rendered source snippets and dependency failures cannot prove
    that the intended API or ownership use was rejected.
    """
    try:
        records = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    except (TypeError, ValueError) as error:
        raise RuntimeError('Cargo did not return valid diagnostic JSON') from error
    if any(not isinstance(record, dict) for record in records):
        raise RuntimeError('invalid Cargo diagnostic record')
    errors = [record for record in records if record.get('reason') == 'compiler-message'
              and record.get('message', {}).get('level') == 'error']
    if success:
        if result.returncode != 0 or errors:
            raise RuntimeError('consumer did not build successfully\n' + result.stdout + result.stderr)
        return []
    if result.returncode == 0 or not errors or diagnostic is None:
        raise RuntimeError('missing expected consumer compiler rejection')
    expected_code, subject = diagnostic
    accepted = []
    for record in errors:
        message = record['message']
        code = (message.get('code') or {}).get('code')
        text = message.get('message', '')
        owner = Path(record.get('target', {}).get('src_path', '')).resolve()
        manifest_dir = Path(record.get('manifest_path', source.parent.parent / 'Cargo.toml')).parent
        primary_in_consumer = any(span.get('is_primary')
                                  and (manifest_dir / span.get('file_name', '')).resolve() == source.resolve()
                                  for span in message.get('spans', []))
        # Const-generic errors originate in the library's assert macro while
        # monomorphizing this consumer; Cargo must still attribute them to it.
        if (owner != source.resolve() or code != expected_code or subject not in text
                or (code != 'E0080' and not primary_in_consumer)):
            raise RuntimeError(f'unexpected consumer rejection: {code}: {text}')
        accepted.append({'code': code, 'message': text})
    return accepted


def consumer(name, features, body, success=True, diagnostic=None, helper=False):
    directory = OUT / name
    (directory / 'src').mkdir(parents=True, exist_ok=True)
    deps = '\n'.join(f'{crate} = {{ path = {json.dumps(str(ROOT / "crates" / crate))}' +
                     (', default-features = false, features = ' + json.dumps(features) if crate == 'moteframe-ui' else '') + ' }'
                     for crate in ['moteframe-ui', 'moteframe-core'])
    if helper:
        dep = directory / 'helper'
        (dep / 'src').mkdir(parents=True, exist_ok=True)
        (dep / 'Cargo.toml').write_text('[package]\nname="feature-helper"\nversion="0.0.0"\nedition="2024"\n[dependencies]\n' +
                                       'moteframe-ui = { path = ' + json.dumps(str(ROOT / 'crates/moteframe-ui')) + ', default-features=false, features=["layout","frame-pacing"] }\n')
        (dep / 'src/lib.rs').write_text('#![no_std]\npub use moteframe_ui::FrameInterval;\n')
        deps += '\nfeature-helper = { path="helper" }'
    (directory / 'Cargo.toml').write_text('[package]\nname="ui-consumer"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\n' + deps + '\n')
    (directory / 'src/lib.rs').write_text(COMMON + body)
    subprocess.run(['cargo','generate-lockfile','--manifest-path',str(directory/'Cargo.toml'),'--offline'],check=True,capture_output=True,text=True)
    result = subprocess.run(['cargo', 'build', '--locked', '--message-format=json', '--manifest-path', str(directory / 'Cargo.toml'), '--offline', '--target', 'thumbv6m-none-eabi', '--target-dir', str(OUT / 'build')], capture_output=True, text=True)
    log = result.stdout + result.stderr
    (directory / 'result.log').write_text(log)
    try:
        errors = validate_build(result, directory / 'src/lib.rs', success, diagnostic)
    except RuntimeError as error:
        raise RuntimeError(f'{name}: {error}\n{log}') from error
    return {'name': name, 'features': features, 'success_expected': success,
            'diagnostic': diagnostic, 'matched_errors': errors}


def main():
    report = []
    features = ['layout', 'refresh', 'damage', 'frame-pacing']
    for bits in itertools.product([False, True], repeat=4):
        selected = [feature for feature, enabled in zip(features, bits) if enabled]
        present = effective(selected)
        name = '-'.join(selected) or 'base'
        body = 'pub fn exercise()->Result<(),ui::ConfigError>{\nlet mut state=ui::UiState::new(settings(),Dimensions::new(2,2),ui::ResourceView::empty(&()))?;\n'
        body += 'let _=state.try_update(settings(),ui::ResourceView::empty(&()))?;\n'
        if 'layout' in present:
            body += 'let _=state.context(ui::ResourceView::empty(&()))?.stack(ui::Axis::Vertical);\n'
        if 'refresh' in present:
            body += 'let mut pixels=[C::Off;2];let mut session=ui::RefreshSession::new(&mut state,&mut pixels,ui::RefreshSettings::new())?;let mut screen=Screen;\n'
            body += 'let _=session.' + ('draw_timed' if 'frame-pacing' in present else 'draw') + '(ui::DisplayBinding::new(&mut screen,1),ui::ResourceView::empty(&()),' + ('||0,' if 'frame-pacing' in present else '') + '|_,_|Ok::<_,()>(()),|_|Ok::<_,()>(()));\n'
        if 'damage' in present:
            body += 'let mut rows=[C::Off;4];let _=ui::damage_cooperative(Dimensions::new(2,2),&mut rows,C::Off,|_|Ok::<_,()>(()),|_|Ok::<_,()>(()),||Ok::<_,()>(()));\n'
        body += 'Ok(())}\n'
        fixture = OUT / name
        fixture.mkdir(parents=True, exist_ok=True)
        flags = ['--no-default-features'] + (['--features', ','.join(selected)] if selected else [])
        runtime = subprocess.run(['cargo','test','-p','moteframe-ui','--all-targets','--offline','--locked',*flags], cwd=ROOT, capture_output=True, text=True)
        (fixture/'runtime.log').write_text(runtime.stdout+runtime.stderr)
        if runtime.returncode:
            raise RuntimeError(name + ': runtime subset failed\n' + runtime.stdout + runtime.stderr)
        report.append(consumer(name, selected, body))
    absent = [('base-no-layout', [], 'use ui::LayoutSettings;', ('E0432', 'LayoutSettings')),
              ('base-no-refresh', [], 'use ui::RefreshSession;', ('E0432', 'RefreshSession')),
              ('refresh-no-damage', ['refresh'], 'use ui::damage_cooperative;', ('E0432', 'damage_cooperative')),
              ('default-no-pacing', ['layout','refresh','damage'], 'use ui::FrameInterval;', ('E0432', 'FrameInterval')),
              ('base-no-layout-field', [], 'fn bad(){let _=settings().with_layout(());}', ('E0599', 'with_layout'))]
    for name, selected, body, diagnostic in absent:
        report.append(consumer(name, selected, body, False, diagnostic))
    start = 'pub fn bad()->Result<(),ui::ConfigError>{let mut state=ui::UiState::new(settings(),Dimensions::new(2,2),ui::ResourceView::empty(&()))?;let mut pixels=[C::Off;2];'
    cases = [
        ('borrow-state', start+'let session=ui::RefreshSession::new(&mut state,&mut pixels,ui::RefreshSettings::new())?;let _=state.try_update(settings(),ui::ResourceView::empty(&()));let _=session.pending();Ok(())}', ('E0499', 'state')),
        ('borrow-workspace', start+'let session=ui::RefreshSession::new(&mut state,&mut pixels,ui::RefreshSettings::new())?;pixels[0]=C::On;let _=session.pending();Ok(())}', ('E0506', 'pixels')),
        ('private-settings', 'fn bad(){let mut s=settings();s.resources=ui::ResourceKey::EMPTY;}', ('E0616', 'resources')),
        ('wrong-color', start+'let mut wrong=[ui::graphics::pixelcolor::Gray8::new(0);2];let _=ui::RefreshSession::new(&mut state,&mut wrong,ui::RefreshSettings::new());Ok(())}', ('E0308', 'mismatched types')),
        ('resource-lifetime', 'fn bad(){let data=[1u8];let view=ui::ResourceView::empty(&data);let escaped;{let local=ui::UiState::new(settings(),Dimensions::new(2,2),view).unwrap();escaped=local.context(view);}let _=escaped;}', ('E0597', 'local')),
        ('provider-lifetime', 'fn bad(){let state=ui::UiState::new(settings(),Dimensions::new(2,2),ui::ResourceView::empty(&())).unwrap();let escaped;{let local=[1u8];escaped=state.context(ui::ResourceView::empty(&local));}let _=escaped;}', ('E0597', 'local')),
        ('run-zero', 'pub fn bad(){let mut d=Screen;let _=ui::StreamTarget::<_,0>::with_run(&mut d);}', ('E0080', 'RUN must be 1..=65535')),
        ('run-overflow', 'pub fn bad(){let mut d=Screen;let _=ui::StreamTarget::<_,65536>::with_run(&mut d);}', ('E0080', 'RUN must be 1..=65535')),
    ]
    for name, body, diagnostic in cases:
        report.append(consumer(name, features, body, False, diagnostic))
    report.append(consumer('feature-unification', [], 'pub fn merged()->Result<(),ui::ConfigError>{let _=ui::LayoutSettings::new();let _=ui::FrameInterval::Unlimited;let _=feature_helper::FrameInterval::Unlimited;let mut state=ui::UiState::new(settings(),Dimensions::new(2,2),ui::ResourceView::empty(&()))?;let mut pixels=[C::Off;2];let mut session=ui::RefreshSession::new(&mut state,&mut pixels,ui::RefreshSettings::new())?;let mut d=Screen;let _=session.draw(ui::DisplayBinding::new(&mut d,1),ui::ResourceView::empty(&()),|_,_|Ok::<_,()>(()),|_|Ok::<_,()>(()));Ok(())}', helper=True))
    path = ROOT / 'target/ui-config-implementation/features.json'
    path.write_text(json.dumps(report, indent=2)+'\n')
    print(f'UI consumers: {len(report)} passed (16 feature subsets, absent APIs, ownership and feature unification).')


if __name__ == '__main__':
    main()
