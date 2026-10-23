// Deterministic controller DOM. Actual layout, CSP and playback need Chromium.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mountFlash } from '../../apps/public/static/native-swf.v1.js';
class Node {
  constructor(tag='div') { this.tagName=tag; this.children=[]; this.dataset={}; this.style={}; this.attrs=new Map(); this.events=new Map(); this.textContent=''; }
  append(...nodes) { for(let node of nodes) { if(typeof node==='string') node=Object.assign(new Node('#text'),{textContent:node}); node.parentElement=this; this.children.push(node); } }
  prepend(node) { node.parentElement=this; this.children.unshift(node); }
  after(node) { node.parentElement=this.parentElement; const list=this.parentElement.children; list.splice(list.indexOf(this)+1,0,node); }
  remove() { if(this.parentElement) { const list=this.parentElement.children; list.splice(list.indexOf(this),1); this.parentElement=null; } }
  get isConnected() { return this.root===true || this.parentElement?.isConnected===true; }
  setAttribute(name,value) { this.attrs.set(name,String(value)); }
  getAttribute(name) { return this.attrs.get(name)??null; }
  addEventListener(name,fn) { if(!this.events.has(name))this.events.set(name,new Set());this.events.get(name).add(fn); }
  removeEventListener(name,fn) { this.events.get(name)?.delete(fn); }
  emit(name,extra={}) { const event={target:this,preventDefault(){this.prevented=true;},...extra};for(const fn of this.events.get(name)??[])fn(event);return event; }
  querySelector(selector) { return this.find(node=>selector==='a[data-player]'?node.tagName==='a'&&node.dataset.player:selector==='[data-swf-toggle]'?node.dataset.swfToggle:false); }
  find(match) { for(const node of this.children) { if(match(node))return node;const found=node.find(match);if(found)return found; } return null; }
  scrollIntoView() { this.scrolled=true; }
}
function fixture({thread='123',mobile=false,width=640,height=480,player='https://media.example/player/f/1700000000000'}={}) {
  const body=new Node('body');body.root=true;
  const board=new Node();board.className='board';body.append(board);
  const context=Object.assign(new Node(),{id:'watcher-context',dataset:{uploadBoard:'true',board:'f',thread,mediaOrigin:'https://media.example'}});body.append(context);
  const link=Object.assign(new Node('a'),{href:'https://media.example/f/owned.swf',dataset:{player,width:String(width),height:String(height)}});link.setAttribute('title','owned.swf');
  const info=Object.assign(new Node(),{id:`fT${thread}`});info.append(link);
  const message=Object.assign(new Node(),{id:`m${thread}`});const target=Object.assign(new Node(),{id:`t${thread}`});board.append(info,message,target);
  const table=new Node('table'),heading=new Node('tr'),row=new Node('tr');
  for(let i=0;i<9;i++){heading.append(new Node('td'));row.append(new Node('td'));}
  if(thread==='0') { link.remove();row.children[2].append(link); }
  table.append(heading,row);table.rows=table.children;board.append(table);
  const document={body,documentElement:{clientWidth:mobile?390:1280,clientHeight:900},createElement:tag=>new Node(tag),getElementById:id=>body.find(node=>node.id===id),querySelector:selector=>selector==='.board'?board:selector==='table.flashListing'?table:null};
  const window=new Node('window'),storage=new Map();let mutation;
  Object.assign(window,{localStorage:{getItem:key=>storage.get(key)??null},matchMedia:()=>({matches:mobile}),location:new URL('https://board.example/f/'),getComputedStyle:()=>({getPropertyValue:()=>''}),MutationObserver:class { constructor(fn){mutation=fn;}observe(){}disconnect(){} }});
  return {document,window,link,info,message,target,table,storage,mutation:()=>mutation?.(),frames:()=>{const found=[];function visit(node){if(node.tagName==='iframe')found.push(node);for(const child of node.children)visit(child);}visit(body);return found;}};
}
test('thread Embed is lazy, sandboxed and removed on toggle, pagehide or post replacement',()=>{
  const f=fixture(),controller=mountFlash(f);assert.ok(controller);assert.equal(f.frames().length,0);
  const toggle=f.info.querySelector('[data-swf-toggle]');assert.equal(toggle.textContent,'Embed');
  toggle.emit('click');assert.equal(f.frames().length,1);assert.equal(toggle.textContent,'Remove');
  assert.equal(f.frames()[0].attrs.get('sandbox'),'allow-scripts');assert.equal(f.frames()[0].width,'640');assert.ok(f.target.scrolled);
  toggle.emit('click');assert.equal(f.frames().length,0);assert.equal(toggle.textContent,'Embed');
  toggle.emit('click');f.window.emit('pagehide');assert.equal(f.frames().length,0);
  toggle.emit('click');f.info.remove();f.mutation();assert.equal(f.frames().length,0);controller.destroy();
});
test('desktop listing adds Embed and closes its single overlay without changing download links',()=>{
  const f=fixture({thread:'0'}),href=f.link.href;mountFlash(f);assert.equal(f.table.rows[0].children.length,10);
  const control=f.table.rows[1].children[3].children.find(node=>node.tagName==='a');assert.equal(control.textContent,'Embed');
  control.emit('click');control.emit('click');assert.equal(f.frames().length,1);assert.equal(f.link.href,href);
  const overlay=f.document.getElementById('swf-embed');overlay.emit('click');assert.equal(f.frames().length,0);
  control.emit('click');const icon=f.document.getElementById('swf-embed-close');icon.emit('keydown',{key:'Enter'});assert.equal(f.frames().length,0);
});
test('mobile listing stays download-only and thread dimensions use source scaling',()=>{
  const index=fixture({thread:'0',mobile:true});mountFlash(index);assert.equal(index.table.rows[0].children.length,9);assert.equal(index.frames().length,0);
  const thread=fixture({mobile:true});mountFlash(thread);thread.info.querySelector('[data-swf-toggle]').emit('click');assert.equal(thread.frames()[0].width,'360');assert.equal(thread.frames()[0].height,'270');
});
test('disabled settings, invalid targets and excessive display surfaces never load a movie',()=>{
  const disabled=fixture();disabled.storage.set('4chan-settings','{"disableAll":true}');assert.equal(mountFlash(disabled),null);
  for(const player of ['https://other.example/player/f/1','https://media.example/player/g/1','https://media.example/player/f/1?x=1','https://user@media.example/player/f/1']) {
    const f=fixture({player});mountFlash(f);assert.equal(f.info.querySelector('[data-swf-toggle]'),null);assert.equal(f.frames().length,0);
  }
  const tall=fixture({width:1,height:107374182});mountFlash(tall);tall.info.querySelector('[data-swf-toggle]').emit('click');assert.equal(tall.frames().length,0);
});
