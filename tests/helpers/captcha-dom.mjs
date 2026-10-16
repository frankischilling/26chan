// Minimal deterministic DOM/timers for the CAPTCHA controller. Browser layout,
// keyboard behavior and CSP stay in the separate real-browser suite.
export class Element {
  constructor(tag, doc) { this.tagName=tag.toUpperCase();this.ownerDocument=doc;this.children=[];this.parentElement=null;this.dataset={};this.style={};this.attributes=new Map();this.listeners=new Map();this.value='';this.className='';this.hidden=false;this.disabled=false;this.required=false;this.textContent=''; }
  append(...nodes) { for(const node of nodes) {node.parentElement=this;this.children.push(node);} }
  setAttribute(name,value) {this.attributes.set(name,String(value));}
  getAttribute(name) {return this.attributes.get(name)??null;}
  removeAttribute(name) {this.attributes.delete(name);}
  set src(value) {this.setAttribute('src',value);} get src() {return this.getAttribute('src');}
  closest(tag) {for(let node=this;node;node=node.parentElement) if(node.tagName===tag.toUpperCase()) return node;return null;}
  querySelector(selector) {
    const match=node=>selector.startsWith('.') ? node.className.split(' ').includes(selector.slice(1))
      : selector.startsWith('[name=') ? node.name===selector.slice(7,-2)
      : selector==='button[id$="t-load"]' ? node.tagName==='BUTTON' && node.id?.endsWith('t-load')
      : selector==='button[id$="t-help"]' ? node.tagName==='BUTTON' && node.id?.endsWith('t-help') : false;
    const visit=node=>{for(const child of node.children) {if(match(child)) return child;const found=visit(child);if(found) return found;}return null;};return visit(this);
  }
  addEventListener(type,fn) {if(!this.listeners.has(type))this.listeners.set(type,new Set());this.listeners.get(type).add(fn);}
  removeEventListener(type,fn) {this.listeners.get(type)?.delete(fn);}
  emit(type,extra={}) {const event={defaultPrevented:false,preventDefault(){this.defaultPrevented=true;},stopImmediatePropagation(){this.stopped=true;},...extra};for(const fn of this.listeners.get(type)??[]) {fn(event);if(event.stopped)break;}return event;}
  focus() {this.ownerDocument.activeElement=this;}
}
export function environment(fetcher) {
  let clock=0,sequence=0;const timers=new Map(),storage=new Map(),events=new Element('window',null);
  const doc={createElement(tag){return new Element(tag,doc);},activeElement:null};
  const win={Date:{now:()=>clock},fetch:fetcher,alert(){},localStorage:{getItem:key=>storage.get(key)??null,setItem:(key,value)=>storage.set(key,String(value))},
    setTimeout(fn,delay){const id=++sequence;timers.set(id,{fn,at:clock+delay,repeat:0});return id;},
    clearTimeout:id=>timers.delete(id),setInterval(fn,delay){const id=++sequence;timers.set(id,{fn,at:clock+delay,repeat:delay});return id;},clearInterval:id=>timers.delete(id),
    addEventListener:(...args)=>events.addEventListener(...args),removeEventListener:(...args)=>events.removeEventListener(...args),emit:(...args)=>events.emit(...args)};
  const advance=ms=>{const end=clock+ms;for(;;){const next=[...timers].filter(([,timer])=>timer.at<=end).sort((a,b)=>a[1].at-b[1].at)[0];if(!next)break;const [id,timer]=next;clock=timer.at;if(timer.repeat)timer.at+=timer.repeat;else timers.delete(id);timer.fn();}clock=end;};
  return {doc,win,advance,storage,timers};
}
export const settle=()=>new Promise(resolve=>setImmediate(resolve));
