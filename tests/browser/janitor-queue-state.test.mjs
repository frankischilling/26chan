import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync('apps/staff/static/request-queue.js', 'utf8');
function fixture({ storageThrows = false, initialStorage = {}, preferences = false } = {}) {
  let active = null;
  const storage = new Map(Object.entries(initialStorage));
  const timers = new Map(), requests = [], events = new Map(), windowEvents = new Map();
  let sequence = 0;
  class Element {
    constructor(id = '') {
      this.id = id; this.hidden = false; this.value = ''; this.textContent = ''; this.checked = false;
      this.listeners = new Map(); this.classes = new Set(); this.dataset = {}; this.style = {}; this.attributes = new Map(); this.children = []; this.parentElement = null;
      this.classList = { add: name => this.classes.add(name), remove: name => this.classes.delete(name), contains: name => this.classes.has(name), toggle: (name, on) => on ? this.classes.add(name) : this.classes.delete(name) };
    }
    addEventListener(name, handler) { this.listeners.set(name, handler); }
    fire(name) { this.listeners.get(name)?.({ preventDefault() {} }); }
    getAttribute(name) {return this.attributes.get(name) ?? null;}
    setAttribute(name,value) {this.attributes.set(name,value);}
    removeAttribute(name) {this.attributes.delete(name); delete this[name];}
    appendChild(child) {this.children.push(child);child.parentElement=this;return child;}
    remove() {if(this.parentElement)this.parentElement.children=this.parentElement.children.filter(child=>child!==this);this.parentElement=null;}
    closest() { return null; }
    matches() { return false; }
    querySelector() { return null; }
    focus() { active = this; }
    getBoundingClientRect() { return { top: 0, bottom: 50, left: 100, right: 150 }; }
    scrollIntoView() {}
  }
  class Form extends Element {
    constructor(row) {
      super(); this.row = row; this.action = 'http://localhost:3001/ban-requests/review';
      this.values = [['id', row.id], ['csrf', 'synthetic']];
    }
    reportValidity() { return true; }
    closest(selector) { return selector === '.ban-request' ? this.row : null; }
  }
  const ids = new Map(['filter-form', 'search-box', 'queue-status', 'queue-undo', 'cfg-cb-dt', 'queue-keybinds', 'queue-preferences', 'reset-btn', 'content', 'title', 'refresh-btn', 'board-menu'].map(id => [id, new Element(id)]));
  ids.get('content').dataset.board = 'g';
  const preferenceFields = [], boardItems = [];
  if (preferences) {
    for (const id of ['queue-custom-menu','queue-save-preferences','queue-reset-preferences','queue-more-boards','queue-image-hover','queue-hide-thumbnails']) ids.set(id,new Element(id));
    for (const action of ['arrowright','arrowleft','a','d','w','c','r','u','i','escape']) { const field=new Element(); field.dataset.queueKey=action; preferenceFields.push(field); }
    for (const board of ['g','a','jp']) { const item=new Element(); item.dataset.board=board; boardItems.push(item); }
  }

  const rows = ['Alpha', 'Beta', 'Gamma'].map((word, index) => {
    const row = new Element(String(index + 1));
    row.fields = { '.post-name': { textContent: word }, '.post-content': { textContent: 'owned' }, '.ban-request-template': { textContent: 'Spam' } };
    row.querySelector = selector => row.fields[selector] || null;
    row.form = new Form(row);
    return row;
  });
  const root = new Element(); root.clientHeight = 900; root.clientWidth = 1200;
  const body = new Element();
  const document = {
    documentElement: root, body, createElement: tag => { const element=new Element(); if(tag==='video'){element.play=()=>Promise.resolve(); element.pause=()=>{};element.load=()=>{};} return element; }, getElementById: id => ids.get(id),
    addEventListener: (event, callback) => events.set(event, callback),
    querySelectorAll: selector => selector === '#br-list .ban-request' ? rows : selector === '[data-queue-key]' ? preferenceFields : selector === '#board-menu .board-slug' ? boardItems : [],
  };
  const context = vm.createContext({
    document, Element, HTMLFormElement: Form,
    FormData: class { constructor(form) { this.values = form.values; } [Symbol.iterator]() { return this.values[Symbol.iterator](); } },
    URL, URLSearchParams, AbortController,
    location: { href: 'http://localhost:3001/ban-requests?board=g', reload() {} },
    window: { addEventListener: (event, callback) => windowEvents.set(event, callback) },
    localStorage: { getItem(key) { if (storageThrows) throw Error(); return storage.get(key) ?? null; }, setItem(key, value) { if (storageThrows) throw Error(); storage.set(key, value); }, removeItem(key) { if (storageThrows) throw Error(); storage.delete(key); } },
    setTimeout: (callback, ms) => { assert([150,3000].includes(ms)); timers.set(++sequence, callback); return sequence; },
    clearTimeout: id => timers.delete(id),
    fetch: (url, options) => new Promise((resolve, reject) => requests.push({ url, options, resolve, reject })),
  });
  vm.runInContext(source, context);
  return {
    element: () => new Element(), input: target => events.get('input')({target}), change: target => events.get('change')({target}),
    body, hover: target => events.get('mouseover')({target}), leave: target => events.get('mouseout')({target}),
    media(index=0,format='png') {const row=rows[index],thumb=new Element(),link=new Element(); thumb.classList.add('post-thumb');link.dataset.format=format;link.setAttribute('href',`/ban-requests/media/${row.id}/file`);thumb.closest=selector=>selector==='.post-thumb-link'?link:selector==='.ban-request'?row:null;row.fields['.post-thumb']=thumb;return {thumb,link};},
    ids, rows, timers, requests, storage, preferenceFields, boardItems, active: () => active,
    key: (key, target = new Element()) => events.get('keydown')({ key, target, preventDefault() {} }),
    submit: (index = 0, decision = 'deny') => events.get('submit')({ target: rows[index].form, submitter: { name: 'decision', value: decision, disabled: false }, preventDefault() {} }),
    run: () => { const callbacks = [...timers.values()]; timers.clear(); for (const callback of callbacks) callback(); },
    hide: () => windowEvents.get('pagehide')(),
  };
}
test('source fields filter loaded requests case-insensitively and reset', () => {
  const f = fixture();
  f.ids.get('search-box').value = 'BETA'; f.ids.get('filter-form').fire('submit');
  assert.deepEqual(f.rows.map(row => row.hidden), [true, false, true]);
  f.ids.get('filter-form').fire('reset');
  assert(f.rows.every(row => !row.hidden));
});
test('focus skips filtered rows, wraps forward and clears before the first row', () => {
  const f = fixture(); f.rows[1].hidden = true;
  f.key('ArrowRight'); assert.equal(f.active(), f.rows[0]);
  f.key('ArrowRight'); assert.equal(f.active(), f.rows[2]);
  f.key('ArrowRight'); assert.equal(f.active(), f.rows[0]);
  f.key('ArrowLeft'); assert(f.rows.every(row => !row.classes.has('focused')));
});
test('pending rows advance focus to the following row rather than wrapping early', () => {
  const f = fixture(); f.key('ArrowRight'); f.key('ArrowRight');
  f.submit(1); assert.equal(f.active(), f.rows[2]);
});
test('review is delayed, repeated submissions do not queue another mutation, and Undo sends nothing', () => {
  const f = fixture(); f.submit(); f.submit();
  assert.equal(f.timers.size, 1); assert.equal(f.requests.length, 0);
  f.ids.get('queue-undo').fire('click'); f.run();
  assert.equal(f.requests.length, 0); assert(!f.rows[0].classes.has('processing'));
});
test('timer sends the immutable submitted values once and prevents a second in-flight review', () => {
  const f = fixture(); f.submit(0, 'accept'); f.rows[0].form.values[0][1] = 'changed'; f.run(); f.submit(1);
  assert.equal(f.requests.length, 1);
  assert.equal(f.requests[0].options.body.get('id'), '1');
  assert.equal(f.requests[0].options.body.get('decision'), 'accept');
  assert.equal(f.requests[0].options.headers.Accept, 'application/json');
});
test('lost response never automatically retries or re-enables stale review controls', async () => {
  const f = fixture(); f.submit(); f.run(); f.requests[0].reject(Error('lost'));
  await new Promise(resolve => setImmediate(resolve));
  f.submit(); f.run(); assert.equal(f.requests.length, 1);
  assert.match(f.ids.get('queue-status').textContent, /Refresh/);
});
test('read refresh is blocked during a pending mutation and navigation clears its timer', () => {
  const f = fixture(); f.submit(); f.ids.get('refresh-btn').fire('click');
  assert.equal(f.requests.length, 0);
  f.hide(); f.run(); assert.equal(f.requests.length, 0);
});
test('unavailable browser storage does not break filtering or review controls', () => {
  const f = fixture({ storageThrows: true });
  f.ids.get('cfg-cb-dt').fire('change'); f.ids.get('queue-keybinds').fire('change');
  f.submit(); f.run(); assert.equal(f.requests.length, 1);
});

test('isolated source filter agrees across names, comments, templates and literal queries', () => {
  const reference = JSON.parse(readFileSync('fixtures/janitor-queue-filter.json', 'utf8'));
  const fields = ['.post-name', '.post-content', '.ban-request-template'];
  const values = ['', 'ALPHA', 'Beta', '<script>owned</script>', 'two words', 'Σigma', 'a\nb', '[literal].*'];
  let count = 0;
  for (const field of fields) for (const text of values) for (const query of values) {
    const f = fixture();
    f.rows[0].fields[field].textContent = text;
    const BR = {};
    const $ = {
      id: id => f.ids.get(id),
      cls: (name, root) => root ? [root.fields['.' + name]].filter(Boolean) : f.rows,
    };
    vm.runInNewContext(reference.functions, { BR, $ });
    BR.applySearch(query);
    const expected = f.rows.map(row => row.classes.has('hidden'));
    BR.clearSearch();
    f.ids.get('search-box').value = query;
    f.ids.get('filter-form').fire('submit');
    assert.deepEqual(f.rows.map(row => row.hidden), expected);
    count++;
  }
  assert.equal(count, 192);
});

test('quick amendments preserve explicit warning and scope through delay and Undo', () => {
  for (const [days, scope] of [['0', ''], ['1', '1'], ['7', '1']]) {
    const f = fixture();
    f.rows[0].form.values.push(['decision', 'accept'], ['amend_tpl', '207'], ['ban_length', days], ['global', scope]);
    f.submit(0, 'accept'); f.run();
    assert.equal(f.requests.length, 1);
    const data = f.requests[0].options.body;
    assert.equal(data.get('amend_tpl'), '207');
    assert.equal(data.get('ban_length'), days);
    assert.equal(data.get('global'), scope);
    const cancelled = fixture();
    cancelled.rows[0].form.values.push(['decision', 'accept'], ['amend_tpl', '207'], ['ban_length', days], ['global', scope]);
    cancelled.submit(0, 'accept'); cancelled.ids.get('queue-undo').fire('click'); cancelled.run();
    assert.equal(cancelled.requests.length, 0);
  }
});

test('W only invokes an enabled warning shortcut on the focused row', () => {
  const f = fixture(); let clicks = 0;
  f.key('w'); assert.equal(clicks, 0);
  f.key('ArrowRight');
  const button = { disabled: false, click() { clicks++; } };
  f.rows[0].fields['button[data-review="Warn"]'] = button;
  f.key('w'); assert.equal(clicks, 1);
  button.disabled = true; f.key('w'); assert.equal(clicks, 1);
  f.ids.get('queue-keybinds').checked = false;
  button.disabled = false; f.key('w'); assert.equal(clicks, 1);
});
test('saved key remapping moves the action and leaves its original key inactive', () => {
  const f=fixture({initialStorage:{'br-keyremap':'[[39,78]]'}});
  f.key('ArrowRight'); assert.equal(f.active(),null); f.key('n'); assert.equal(f.active(),f.rows[0]);
});
test('malformed, duplicate, unsupported and colliding saved mappings retain defaults', () => {
  for(const raw of ['{','{}','[[39,65]]','[[39,78],[39,80]]','[[73,78]]','[[39,49]]','[null]']) {
    const f=fixture({initialStorage:{'br-keyremap':raw}}); f.key('ArrowRight'); assert.equal(f.active(),f.rows[0],raw);
  }
});
test('settings validate all keys before saving and reset restores defaults', () => {
  const f=fixture({preferences:true}), next=f.preferenceFields.find(field=>field.dataset.queueKey==='arrowright');
  next.value='a'; f.ids.get('queue-save-preferences').fire('click'); assert(!f.storage.has('br-keyremap'));
  assert.match(f.ids.get('queue-status').textContent,/different keys/);
  next.value='N'; f.ids.get('queue-save-preferences').fire('click'); assert.equal(f.storage.get('br-keyremap'),'[[39,78]]');
  f.key('n'); assert.equal(f.active(),f.rows[0]); f.ids.get('queue-reset-preferences').fire('click');
  assert(!f.storage.has('br-keyremap')); f.key('ArrowRight'); assert.equal(f.active(),f.rows[1]);
});
test('custom board list, More and reset preserve unrelated source preferences', () => {
  const f=fixture({preferences:true,initialStorage:{'rq-settings':'{"customMenu":"g,jp","imageHover":false}'}});
  assert.deepEqual(f.boardItems.map(item=>item.hidden),[false,true,false]);
  f.ids.get('queue-more-boards').fire('click'); assert(f.boardItems.every(item=>!item.hidden));
  f.ids.get('queue-custom-menu').value='a'; f.ids.get('queue-save-preferences').fire('click');
  assert.deepEqual(f.boardItems.map(item=>item.hidden),[true,false,true]);
  assert.deepEqual(JSON.parse(f.storage.get('rq-settings')),{customMenu:'a',imageHover:false});
  f.ids.get('queue-reset-preferences').fire('click'); assert(f.boardItems.every(item=>!item.hidden));
  assert(!f.storage.has('rq-settings'));
});
function amendmentFixture(f,names) {
  const search=f.element(),select=f.element(),days=f.element(),scope=f.element();
  const options=names.map((name,index)=>({value:String(index+1),textContent:name,dataset:{templateName:name,seconds:String(index*86400),global:index===2?'1':'0'}}));
  select.options=options; select.value='1';
  Object.defineProperty(select,'selectedOptions',{get:()=>options.filter(option=>option.value===select.value)});
  const form={querySelector:selector=>selector==='[name="amend_tpl"]'?select:selector==='[name="ban_length"]'?days:selector==='[name="global"]'?scope:null};
  search.closest=select.closest=()=>form;
  search.matches=selector=>selector==='[data-template-search]'; select.matches=selector=>selector==='.request-amend select[name="amend_tpl"]';
  return {search,select,days,scope,options};
}
test('autocomplete and template changes retain source word-prefix matching and defaults', () => {
  const reference=JSON.parse(readFileSync('fixtures/janitor-preference-source.json')).search;
  const names=['Off topic','Ban Evasion [Short]','Global spam','Spam (repeated)'];
  for(const query of ['','spam','ban short','topic','evasion','(repeated)','no-match',' topic','ban  short']) {
    const f=fixture(),{search,select,days,scope,options}=amendmentFixture(f,names);
    const original={children:options,selectedIndex:0};
    const context=vm.createContext({BR:{onAmendChange(){}},$:{id:id=>id==='amend-auto'?{value:query}:original}});
    vm.runInContext(reference.fragment,context); vm.runInContext('BR.onAmendKeyUp.call({}, {keyCode:65})',context);
    search.value=query; f.input(search); assert.equal(select.value,options[original.selectedIndex].value,query);
    select.value='3'; f.change(select); assert.equal(days.value,'2'); assert.equal(scope.value,'1');
  }
});
test('autocomplete treats regex control characters literally and skips disabled choices', () => {
  const f=fixture(),{search,select,days,options}=amendmentFixture(f,['Spam','Spam']);
  options[0].disabled=true; options[1].dataset.seconds='86401'; select.value='old';
  search.value='.*'; f.input(search); assert.equal(select.value,'old');
  search.value='Spam'; f.input(search); assert.equal(select.value,'2'); assert.equal(days.value,'');
});
test('custom menu decisions match the isolated source including empty comma fields', () => {
  const reference=JSON.parse(readFileSync('fixtures/janitor-preference-source.json')).menu;
  for(const customMenu of ['','g','a,jp','g,,a',',g','g, jp','missing','g,g']) {
    const f=fixture({preferences:true,initialStorage:{'rq-settings':JSON.stringify({customMenu})}});
    const items=['g','a','jp'].map(textContent=>({textContent,style:{display:null}}));
    const context=vm.createContext({BR:{},$:{id:()=>({style:{}}),cls:()=>items}});
    vm.runInContext(reference.fragment,context); context.value=customMenu; vm.runInContext('BR.applyCustomMenu(value)',context);
    assert.deepEqual(f.boardItems.map(item=>item.hidden),items.map(item=>item.style.display==='none'),customMenu);
  }
});
test('source key alphabet and native accepted remaps agree without collisions', () => {
  const reference=JSON.parse(readFileSync('fixtures/janitor-preference-source.json')).keys;
  const context=vm.createContext({Keybinds:{}}); vm.runInContext(reference.fragment,context);
  const reserved=new Set([27,37,39,65,67,68,73,82,85,87]);
  for(let code=0;code<128;code++) { if(reserved.has(code)) continue;
    context.code=code; const supported=Boolean(vm.runInContext('Keybinds.codeToKey(code)',context));
    const f=fixture({initialStorage:{'br-keyremap':JSON.stringify([[39,code]])}});
    f.key('ArrowRight'); assert.equal(f.active()===null,supported,`code ${code}`);
  }
});
test('preference storage failures keep current-page controls usable', () => {
  const f=fixture({preferences:true,storageThrows:true});
  f.preferenceFields.find(input=>input.dataset.queueKey==='arrowright').value='n';
  f.ids.get('queue-custom-menu').value='g'; f.ids.get('queue-save-preferences').fire('click');
  f.key('n'); assert.equal(f.active(),f.rows[0]); assert.deepEqual(f.boardItems.map(item=>item.hidden),[false,true,true]);
});
test('saving a menu does not destroy an oversized unrelated settings record', () => {
  const raw=JSON.stringify({customCSS:'x'.repeat(5000)}),f=fixture({preferences:true,initialStorage:{'rq-settings':raw}});
  f.ids.get('queue-custom-menu').value='g'; f.ids.get('queue-save-preferences').fire('click'); assert.equal(f.storage.get('rq-settings'),raw);
});

test('changing a template restores its inert public reason without carrying an old override', () => {
  const f=fixture(), {select}=amendmentFixture(f,['Owned']);
  const reason=f.element(); reason.value='Previous override';
  const form=select.closest('form'), original=form.querySelector;
  form.querySelector=selector=>selector==='[name="public_reason"]'?reason:original(selector);
  f.ids.set('queue-template-reasons',{content:{children:[{dataset:{templateId:'1'},textContent:'Owned <script>reason</script>'}]}});
  f.change(select); assert.equal(reason.value,'Owned <script>reason</script>');
});

test('retained thumbnails expand after the source hover delay and close on exit or navigation', () => {
  const f=fixture(), {thumb}=f.media();
  f.hover(thumb); assert.equal(f.body.children.length,0); assert.equal(f.timers.size,1);
  f.run(); assert.equal(f.body.children.length,1); assert.equal(f.body.children[0].src,'/ban-requests/media/1/file');
  f.leave(thumb); assert.equal(f.body.children.length,0);
  f.hover(thumb); f.hide(); assert.equal(f.timers.size,0); f.run(); assert.equal(f.body.children.length,0);
});
test('I toggles focused media and navigation, filtering and pending review close it', () => {
  const f=fixture(); f.media(); f.key('ArrowRight'); f.key('i'); assert.equal(f.body.children.length,1);
  f.key('i'); assert.equal(f.body.children.length,0); f.key('i'); f.key('ArrowRight'); assert.equal(f.body.children.length,0);
  f.key('ArrowLeft'); f.key('i'); f.ids.get('filter-form').fire('submit'); assert.equal(f.body.children.length,0);
  f.key('i'); f.submit(0); assert.equal(f.body.children.length,0);
});
test('media preferences preserve source defaults and reset owned settings', () => {
  const f=fixture({preferences:true, initialStorage:{'rq-settings':'{"imageHover":false,"hideThumbnails":true,"customCSS":"owned"}'}}), {thumb}=f.media();
  assert(f.body.classes.has('no-thumbnails')); f.hover(thumb); assert.equal(f.timers.size,0);
  f.ids.get('queue-reset-preferences').fire('click'); assert(!f.body.classes.has('no-thumbnails'));
  assert.deepEqual(JSON.parse(f.storage.get('rq-settings')),{customCSS:'owned'}); f.hover(thumb); assert.equal(f.timers.size,1);
});
test('expansion refuses documents, external targets, processing rows and stale errors', () => {
  const f=fixture(), {thumb,link}=f.media(0,'pdf'); f.hover(thumb); f.run(); assert.equal(f.body.children.length,0);
  link.dataset.format='png'; link.setAttribute('href','https://example.invalid/private'); f.hover(thumb);f.run();assert.equal(f.body.children.length,0);
  link.setAttribute('href','/ban-requests/media/1/file'); f.rows[0].classList.add('processing'); f.hover(thumb); f.run(); assert.equal(f.body.children.length,0);
  f.rows[0].classList.remove('processing');f.hover(thumb);f.run();const old=f.body.children[0];f.leave(thumb);f.hover(thumb);f.run();old.onerror();assert.equal(f.body.children.length,1);
  f.body.children[0].onerror();assert.equal(f.body.children.length,0);assert.match(f.ids.get('queue-status').textContent,/unavailable/);
});
