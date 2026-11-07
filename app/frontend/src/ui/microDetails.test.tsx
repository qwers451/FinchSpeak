import { createSignal } from 'solid-js';
import { render } from 'solid-js/web';
import { afterEach, expect, it, vi } from 'vitest';
import { Button } from './Button';
import { Slider } from './Slider';
import { ToastHost } from './ToastHost';
import { clearToasts, dismissToast, pushToast, toasts } from './toastStore';
let dispose: (() => void) | undefined;
function mount(view: () => any) { const host=document.createElement('div'); document.body.append(host); dispose=render(view,host); return host; }
afterEach(()=>{ dispose?.(); clearToasts(); document.body.innerHTML=''; delete document.documentElement.dataset.reduceMotion; vi.useRealTimers(); vi.unstubAllEnvs(); vi.unstubAllGlobals(); });
it('preserves the button content and icon while an independent spinner is shown',()=>{
 const [loading,setLoading]=createSignal(false);
 const host=mount(()=> <Button icon="check" loading={loading()}>Save</Button>);
 const content=host.querySelector('.btn-content');
 setLoading(true);
 expect(host.querySelector('.btn-content')).toBe(content);
 expect(content?.textContent).toBe('Save');
 expect(content?.querySelector('svg')).not.toBeNull();
 expect(host.querySelector('.btn-spinner')?.getAttribute('aria-hidden')).toBe('true');
 expect(host.querySelector('button')?.disabled).toBe(true);
 setLoading(false); expect(host.querySelector('.btn-spinner')).toBeNull();
});
it('keeps the native range input and updates the visual value on input',()=>{
 const [value,setValue]=createSignal(50);
 const host=mount(()=> <Slider value={value()} min={0} max={200} label="Volume" onChange={setValue}/>);
 const input=host.querySelector('input')!; input.value='125'; input.dispatchEvent(new Event('input',{bubbles:true}));
 expect(value()).toBe(125); expect(host.querySelector('.range-value')?.textContent).toBe('125');
 expect(input.getAttribute('aria-label')).toBe('Volume');
});
it('disables leaving toast actions immediately, then removes the visual after 140ms',()=>{
 vi.stubEnv('MODE','development'); vi.useFakeTimers();
 const run=vi.fn(); const host=mount(()=> <ToastHost/>);
 pushToast({ title:'Saved', action:{label:'Retry',run} });
 const action=host.querySelector('.link-btn') as HTMLButtonElement; action.click(); action.click();
 expect(run).toHaveBeenCalledTimes(1); expect(toasts()).toHaveLength(0);
 expect(host.querySelector('.toast')?.hasAttribute('inert')).toBe(true);
 expect(host.querySelector('[role="status"]')).toBeNull();
 vi.advanceTimersByTime(139); expect(host.querySelector('.toast')).not.toBeNull();
 vi.advanceTimersByTime(1); expect(host.querySelector('.toast')).toBeNull();
});
it.each(['preference','system'])('removes instantly for %s reduced motion',kind=>{
 if(kind==='preference') document.documentElement.dataset.reduceMotion='true';
 if(kind==='system') vi.stubGlobal('matchMedia',()=>({matches:true}));
 const host=mount(()=> <ToastHost/>); const id=pushToast({title:'Saved'}); dismissToast(id);
 expect(host.querySelector('.toast')).toBeNull();
});
it('keeps the exit visual under the test runner: motion is not switched by MODE',()=>{
 vi.stubEnv('MODE','test'); vi.useFakeTimers(); const host=mount(()=> <ToastHost/>);
 dismissToast(pushToast({title:'Saved'}));
 expect(host.querySelector('.toast')?.getAttribute('data-exiting')).toBe('true');
 vi.advanceTimersByTime(139); expect(host.querySelector('.toast')).not.toBeNull();
 vi.advanceTimersByTime(1); expect(host.querySelector('.toast')).toBeNull();
});
it('runs the toast action before closing it',()=>{
 vi.useFakeTimers(); const host=mount(()=> <ToastHost/>);
 let seen=-1;
 pushToast({title:'Saved', action:{label:'Retry', run:()=>{ seen=toasts().length; }}});
 (host.querySelector('.link-btn') as HTMLButtonElement).click();
 expect(seen).toBe(1); expect(toasts()).toHaveLength(0);
});
it('an action that re-pushes the same message keeps that toast visible',()=>{
 vi.useFakeTimers(); const host=mount(()=> <ToastHost/>);
 let again=-1;
 const id=pushToast({title:'Saved', action:{label:'Retry', run:()=>{ again=pushToast({title:'Saved'}); }}});
 (host.querySelector('.link-btn') as HTMLButtonElement).click();
 expect(again).toBe(id); expect(toasts()).toHaveLength(1);
 expect(host.querySelector('.toast')?.hasAttribute('data-exiting')).toBe(false);
 expect(host.querySelector('.toast')).not.toBeNull();
});
it('a new identical toast survives the previous visual exit timer',()=>{
 vi.stubEnv('MODE','development'); vi.useFakeTimers(); const host=mount(()=> <ToastHost/>);
 dismissToast(pushToast({title:'Saved'})); pushToast({title:'Saved'});
 vi.advanceTimersByTime(140); expect(host.querySelectorAll('.toast')).toHaveLength(1); expect(toasts()).toHaveLength(1);
});
