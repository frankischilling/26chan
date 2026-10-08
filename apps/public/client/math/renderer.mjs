import {mathjax} from '@mathjax/src/mjs/mathjax.js';
import {TeX} from '@mathjax/src/mjs/input/tex.js';
import {SVG} from '@mathjax/src/mjs/output/svg.js';
import {liteAdaptor} from '@mathjax/src/mjs/adaptors/liteAdaptor.js';
import {HTMLHandler} from '@mathjax/src/mjs/handlers/html/HTMLHandler.js';
import {Configuration} from '@mathjax/src/mjs/input/tex/Configuration.js';
import {CommandMap} from '@mathjax/src/mjs/input/tex/TokenMap.js';
import Stack from '@mathjax/src/mjs/input/tex/Stack.js';
import TexParser from '@mathjax/src/mjs/input/tex/TexParser.js';
import {ColumnParser} from '@mathjax/src/mjs/input/tex/ColumnParser.js';
import {MmlFactory} from '@mathjax/src/mjs/core/MmlTree/MmlFactory.js';
import BaseMethods from '@mathjax/src/mjs/input/tex/base/BaseMethods.js';
import {MultlineItem,FlalignItem} from '@mathjax/src/mjs/input/tex/ams/AmsItems.js';
import '@mathjax/src/mjs/input/tex/base/BaseConfiguration.js';
import '@mathjax/src/mjs/input/tex/ams/AmsMappings.js';
import {viewBoxNumbers,validateGeometry} from './schema.mjs';

export const RENDER_LIMITS = Object.freeze({
  source: 4096, parsers: 512, depth: 64, mml: 8192, dom: 4096,
  bytes: 1048576, repeat: 256, template: 10240,
});
function fail() {
  throw new Error('Unsupported or oversized math');
}
mathjax.asyncLoad = fail;
let job = null;
function charge(nodes = 0, bytes = 0) {
  if (!job || (job.dom += nodes) > RENDER_LIMITS.dom
      || (job.bytes += bytes) > RENDER_LIMITS.bytes) fail();
}
const forbidden = new Set([
  'def', 'gdef', 'edef', 'xdef', 'let', 'futurelet', 'newcommand', 'renewcommand',
  'providecommand', 'newenvironment', 'renewenvironment', 'newcolumntype',
  'DeclareMathOperator', 'csname', 'endcsname', 'catcode', 'require', 'autoload',
  'includegraphics', 'href', 'url', 'style', 'class', 'cssId', 'htmlId', 'htmlClass',
  'htmlStyle', 'htmlData', 'color', 'textcolor', 'colorbox', 'fcolorbox', 'definecolor',
  'pagecolor', 'bgcolor', 'mmlToken', 'unicode', 'fontsize', 'tiny', 'Tiny',
  'scriptsize', 'SMALL', 'Small', 'footnotesize', 'small', 'normalsize', 'large',
  'Large', 'LARGE', 'huge', 'Huge', 'HUGE', 'displaylines',
]);
// Check expanded control sequences as well as source. Only reviewed built-ins can expand.
const parse=TexParser.prototype.parse;
TexParser.prototype.parse=function(kind,input) {
  if (this.string.length>10240 || kind==='macro' && forbidden.has(input[1])) fail();
  return parse.call(this,kind,input);
};
const parseBody=TexParser.prototype.Parse;
TexParser.prototype.Parse=function(...args) {
  if (!job || ++job.parsers>512 || ++job.depth>64 || this.string.length>10240) fail();
  try {
    return parseBody.apply(this, args);
  } finally {
    job.depth--;
  }
};
const argument=TexParser.prototype.GetArgument;
TexParser.prototype.GetArgument=function(name,...args) {
  const value=argument.call(this,name,...args);
  if (/^\\begin\{(?:alignat\*?|alignedat|xalignat\*?|xxalignat)\}$/.test(name)) {
    const count = Number(value);
    if (!/^\d+$/.test(value) || !Number.isSafeInteger(count) || count < 1 || count > 32) fail();
  }
  return value;
};
ColumnParser.prototype.repeat=function(state) {
  if (++job.repeats>256) fail();
  const raw = this.getBraces(state);
  const cols = this.getBraces(state);
  const n = Number(raw);
  if (!/^\d+$/.test(raw) || !Number.isSafeInteger(n) || n<0 || n>64 || cols.length*n+state.template.length-state.i>10240) fail();
  state.template = cols.repeat(n) + state.template.slice(state.i);
  state.i = 0;
};
const column=ColumnParser.prototype.processColumn;
ColumnParser.prototype.processColumn=function(...args) {
  if (++job.columns>1024 || ++job.columnDepth>64) fail();
  try {
    return column.apply(this, args);
  } finally {
    job.columnDepth--;
  }
};
const pushStack = Stack.prototype.Push;
Stack.prototype.Push = function(...args) {
  if (this.stack.length + args.length > 64) fail();
  return pushStack.apply(this, args);
};
const createMml=MmlFactory.prototype.create;
MmlFactory.prototype.create = function(...args) {
  if (++job.mml > 8192) fail();
  return createMml.apply(this, args);
};
new CommandMap('native-safe-macros',{
  bmod:[BaseMethods.Macro,'\\mathbin{\\mathrm{mod}}'],
  pmod:[BaseMethods.Macro,'\\pod{\\mathrm{mod}\\kern6mu #1}',1],
  mod:[BaseMethods.Macro,'\\quad\\mathrm{mod}\\,\\,#1',1],
  varliminf:[BaseMethods.Macro,'\\mathop{\\underline{\\mathrm{lim}}}'],
  varlimsup:[BaseMethods.Macro,'\\mathop{\\overline{\\mathrm{lim}}}'],
  varinjlim:[BaseMethods.Macro,'\\mathop{\\underrightarrow{\\mathrm{lim}}}'],
  varprojlim:[BaseMethods.Macro,'\\mathop{\\underleftarrow{\\mathrm{lim}}}'],
  injlim:[BaseMethods.Macro,'\\mathop{\\mathrm{inj}\\,\\mathrm{lim}}'],
  projlim:[BaseMethods.Macro,'\\mathop{\\mathrm{proj}\\,\\mathrm{lim}}'],
});
Configuration.create('native-safe', {
  handler: {macro: ['native-safe-macros']}, priority: 1,
});
// Deliberately omit AMS's NewcommandConfig and all extension/component loaders.
Configuration.create('native-ams',{
  handler: {
    character: ['AMSmath-operatorLetter'],
    delimiter: ['AMSsymbols-delimiter', 'AMSmath-delimiter'],
    macro: [
      'AMSsymbols-mathchar0mi', 'AMSsymbols-mathchar0mo', 'AMSsymbols-delimiter',
      'AMSsymbols-macros', 'AMSmath-mathchar0mo', 'AMSmath-macros', 'AMSmath-delimiter',
    ],
    environment: ['AMSmath-environment'],
  },
  items: {
    [MultlineItem.prototype.kind]: MultlineItem,
    [FlalignItem.prototype.kind]: FlalignItem,
  },
  options: {
    multlineWidth: '',
    ams: {operatornamePattern: /^[-*a-zA-Z0-9]+/, multlineWidth: '100%', multlineIndent: '1em'},
  },
});
function boundedAdaptor() {
  const adaptor=liteAdaptor();
  for (const method of ['create','text','comment']) {
    const original=adaptor[method];
    adaptor[method] = function(value, ...args) {
      charge(1, 2 * String(value).length);
      return original.call(this, value, ...args);
    };
  }
  const set=adaptor.setAttribute;
  adaptor.setAttribute = function(node, name, value, ...args) {
    charge(0, 2 * (String(name).length + String(value).length));
    return set.call(this, node, name, value, ...args);
  };
  const style=adaptor.setStyle;
  adaptor.setStyle = function(node, name, value) {
    charge(0, 2 * (String(name).length + String(value).length));
    return style.call(this, node, name, value);
  };
  const split=adaptor.split;
  adaptor.split = function(node, n) {
    charge(1, 2 * node.value.length);
    return split.call(this, node, n);
  };
  // LiteAdaptor.clone constructs elements/text directly; count the entire copy first.
  const clone=adaptor.clone;
  let cloning=false;
  adaptor.clone=function(node,deep=true) {
    if (cloning) return clone.call(this,node,deep);
    function count(n,depth) {
      if (depth>64) fail();
      charge(1,2*(n.value?.length||0));
      for (const [k,v] of Object.entries(n.attributes||{})) charge(0,2*(k.length+String(v).length));
      if (deep) for (const child of n.children||[]) count(child,depth+1);
    }
    count(node, 0);
    cloning = true;
    try {
      return clone.call(this, node, deep);
    } finally {
      cloning = false;
    }
  };
  return adaptor;
}
const metadata=new Set(['data-mml-node','data-mjx-texclass','data-c','data-variant','data-latex','data-latex-item','data-frame-styles','data-frame','data-table','data-align']);
function numeric(value) {
  const number = Number(value);
  if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(String(value))
      || !Number.isFinite(number) || Math.abs(number) > 100000) fail();
  return number;
}
function viewport(attrs) {
  const v=viewBoxNumbers(attrs.viewBox);
  if (!v || v.length!==4 || v[2]<1 || v[3]<1) fail();
  return v;
}
export function extractGeometry(svg) {
  if (svg.kind!=='svg') fail();
  const attrs=svg.attributes;
  for (const key of Object.keys(attrs)) if (!['xmlns','width','height','viewBox','role','focusable','style','preserveAspectRatio'].includes(key)) fail();
  if (attrs.preserveAspectRatio && attrs.preserveAspectRatio!=='xMidYMid meet') fail();
  if (attrs.x!==undefined || attrs.y!==undefined) fail();
  const ex = value => {
    if (!/^(?:\d+(?:\.\d*)?|\.\d+)ex$/.test(value)) fail();
    return Number(value.slice(0, -2));
  };
  const geometry={viewBox:viewport(attrs),width:ex(attrs.width),height:ex(attrs.height),children:[]};
  function node(n,depth) {
    if (depth>60) fail();
    if (n.kind==='path' && n.attributes.d==='') return null;
    if (!['g','path','rect','line','svg'].includes(n.kind)) fail();
    const out={tag:n.kind,attrs:{}};
    if (n.kind==='svg') {
      const a = n.attributes;
      const v = viewport(a);
      const width = numeric(a.width), height = numeric(a.height);
      const x = numeric(a.x ?? 0), y = numeric(a.y ?? 0);
      if (width<0.01 || height<0.01 || (a.preserveAspectRatio && a.preserveAspectRatio!=='xMidYMid meet')) fail();
      for (const key of Object.keys(a)) if (!['x','y','width','height','viewBox','preserveAspectRatio','overflow'].includes(key) && !metadata.has(key)) fail();
      if (a.overflow !== undefined && !['hidden','visible'].includes(a.overflow)) fail();
      // MathJax stretch pieces deliberately overdraw their inner viewport. Keep
      // the viewport itself, not merely its meet transform, to preserve clipping.
      out.attrs={x:String(x),y:String(y),width:String(width),height:String(height),
        viewBox:v.join(' '),preserveAspectRatio:'xMidYMid meet',overflow:a.overflow ?? 'hidden'};
    } else {
      for (const [key,value] of Object.entries(n.attributes)) {
        if (metadata.has(key)) continue;
        out.attrs[key]=String(value);
      }
    }
    if (n.children?.length) out.children=n.children.map(c=>node(c,depth+1)).filter(Boolean);
    return out;
  }
  geometry.children=svg.children.map(n=>node(n,0)).filter(Boolean);
  if (!validateGeometry(geometry)) fail();
  return geometry;
}
export function renderMath(tex,display=false) {
  if (typeof tex!=='string' || !tex.trim() || tex.length>4096 || typeof display!=='boolean' || job) fail();
  let braces=0;
  for (let i = 0; i < tex.length; i++) {
    if (tex[i] === '\\') {
      i++;
      continue;
    }
    if (tex[i] === '{' && ++braces > 64) fail();
    if (tex[i] === '}') braces--;
  }
  job={dom:0,bytes:0,parsers:0,depth:0,mml:0,repeats:0,columns:0,columnDepth:0};
  try {
    const adaptor=boundedAdaptor();
    const input = new TeX({
      packages: ['base', 'native-ams', 'native-safe'],
      maxBuffer: 10240, maxMacros: 512, maxTemplateSubtitutions: 256,
      tags: 'none', formatError: () => fail(),
    });
    input.postFilters.add(({data}) => {
      const pending = [[data.root, 0]];
      let count = 0;
      while (pending.length) {
        const [node, depth] = pending.pop();
        if (++count > 8192 || depth > 64) fail();
        for (const child of node.childNodes || []) {
          // Optional script/accent slots can be absent in the MathML tree.
          if (child != null) pending.push([child, depth + 1]);
        }
      }
    }, -100);
    const output=new SVG({fontCache:'none',useXlink:false,exFactor:0.5});
    output.font.loadDynamicFile=fail;
    output.font.loadDynamicFileSync=fail;
    // No global handler registration; every job gets fresh parser, tags and document state.
    const doc=new HTMLHandler(adaptor).create('',{InputJax:input,OutputJax:output});
    const container=doc.convert(tex,{display,em:16,ex:8,containerWidth:1280});
    const svg=container.children.find(n=>n.kind==='svg');
    return extractGeometry(svg);
  } finally {
    job = null;
  }
}
