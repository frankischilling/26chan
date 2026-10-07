import assert from 'node:assert/strict';
import {test,before,after} from 'node:test';
import {build} from 'esbuild';
import {mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {geometrySize,validateGeometry} from '../../apps/public/client/math/schema.mjs';
let renderMath,extractGeometry,dir;
before(async()=>{
  dir=await mkdtemp(join(tmpdir(),'native-math-'));
  const outfile=join(dir,'renderer.mjs');
  await build({entryPoints:['apps/public/client/math/renderer.mjs'],outfile,bundle:true,platform:'node',format:'esm',logLevel:'silent',alias:{'#default-font/svg/default.js':resolve('node_modules/@mathjax/mathjax-tex-font/mjs/svg/default.js')}});
  ({renderMath,extractGeometry}=await import(pathToFileURL(outfile)));
});
after(async()=>{if(dir) await rm(dir,{recursive:true,force:true});});
for (const tex of [String.raw`x^2+1`,String.raw`\frac{1}{\sqrt{x}}`,String.raw`\sum_{i=1}^{n}i`,String.raw`\text{hello world}`,String.raw`\varliminf x`,String.raw`\varlimsup x`,String.raw`\injlim x`,String.raw`\projlim x`,String.raw`x\pmod{2}+x\bmod y`,String.raw`\begin{array}{*{2}{c}}a&b\\c&d\end{array}`,String.raw`\begin{alignat}{2}a&=b&c&=d\end{alignat}`,String.raw`\underline{abc}`,String.raw`\left(\frac{\frac{a}{b}}{\frac{c}{d}}\right)`]) {
  test(`actual local SVG: ${tex}`,()=>{
    const geometry=renderMath(tex,true);assert.ok(validateGeometry(geometry));
    assert.ok(geometrySize(geometry).nodes>1);assert.match(JSON.stringify(geometry),/"path"/);
    assert.doesNotMatch(JSON.stringify(geometry),/href|<|"style"|"use"|"text"/);
  });
}
for (const tex of [String.raw`\def\x{a}\x`,String.raw`\let\a\frac`,String.raw`\require{html}`,String.raw`\href{https://example.com}{x}`,String.raw`\color{red}x`,String.raw`\mmlToken{mi}[href="x"]{x}`,String.raw`\newcolumntype{A}{c}`,String.raw`\fontsize{100000}{1}x`,String.raw`\SMALL x`,String.raw`\HUGE x`,String.raw`\footnotesize x`,String.raw`\begin{array}{*{999999999}{c}}x\end{array}`,String.raw`\begin{array}{*{64}{*{64}{*{64}{c}}}}x\end{array}`,String.raw`\begin{alignat}{999999999}x\end{alignat}`,String.raw`\begin{alignedat}{0}x\end{alignedat}`,String.raw`\begin{xalignat}{999999999}x\end{xalignat}`,'{'.repeat(65)+'x'+'}'.repeat(65),'x'.repeat(4097)]) {
  test(`reject and recover: ${tex.slice(0,80)}`,()=>{assert.throws(()=>renderMath(tex));assert.ok(validateGeometry(renderMath('a+b')));});
}
test('schema rejects untrusted metadata, markup and unbounded geometry',()=>{
  const good={viewBox:[0,-10,100,100],width:2,height:2,children:[{tag:'path',attrs:{d:'M0 0L10 10'}}]};
  assert.ok(validateGeometry(good));
  for(const patch of [{tag:'script'},{attrs:{d:'M0 0',href:'evil'}},{attrs:{d:'M0 0',style:'color:red'}},{attrs:{d:'M0 0',fill:'url(x)'}},{attrs:{d:'M0 0',transform:'translate(1e99,0)'}}]) assert.equal(validateGeometry({...good,children:[{...good.children[0],...patch}]}),false);
  assert.equal(validateGeometry({...good,viewBox:[0,0,0.000001,1]}),false);
});
test('nested SVG preserves bounded meet viewports and default clipping',()=>{
  const geometry=extractGeometry({kind:'svg',attributes:{width:'2ex',height:'2ex',viewBox:'0 0 100 100'},children:[{kind:'svg',attributes:{x:'10',y:'20',width:'100',height:'50',viewBox:'0 0 10 10'},children:[{kind:'path',attributes:{d:'M0 0L10 10'},children:[]}]}]});
  assert.equal(geometry.children[0].tag,'svg');
  assert.deepEqual(geometry.children[0].attrs,{x:'10',y:'20',width:'100',height:'50',viewBox:'0 0 10 10',preserveAspectRatio:'xMidYMid meet',overflow:'hidden'});
  for (const changes of [{width:'0.0000001'},{width:'1e99'},{viewBox:'0 0 .00001 1'},{x:'Infinity'},{x:'0x10'},{width:'100%'},{overflow:'inherit'},{overflow:'url(x)'},{preserveAspectRatio:'none'},{href:'https://example.com'},{style:'overflow:visible'},{transform:'scale(2)'}]) {
    assert.throws(()=>extractGeometry({kind:'svg',attributes:{width:'2ex',height:'2ex',viewBox:'0 0 100 100'},children:[{kind:'svg',attributes:{x:'0',y:'0',width:'100',height:'50',viewBox:'0 0 10 10',...changes},children:[]}]}));
  }
});

test('nested SVG retains explicit visible overflow and overdraw geometry', () => {
  const children = [{kind:'path',attributes:{d:'M-100 -100L200 200'},children:[]}];
  for (const overflow of ['visible','hidden']) {
    const geometry = extractGeometry({kind:'svg',attributes:{width:'2ex',height:'2ex',viewBox:'0 0 100 100'},children:[{kind:'svg',attributes:{width:'100',height:'50',viewBox:'0 0 10 10',overflow},children}]});
    assert.equal(geometry.children[0].attrs.overflow,overflow);
    assert.equal(geometry.children[0].children[0].attrs.d,'M-100 -100L200 200');
  }
});

test('actual MathJax stretched delimiters and arrows retain clipping viewports', () => {
  // Six full-sized rows exceed the font's largest precomposed (2.999 em)
  // parenthesis and force top/extender/bottom assembly.
  for (const tex of [String.raw`\left(\begin{matrix}a\\b\\c\\d\\e\\f\end{matrix}\right)`, String.raw`\overrightarrow{abcdefghijklmnop}`]) {
    const geometry = renderMath(tex,true);
    const viewports = [];
    function visit(node) {
      if (node.tag === 'svg') viewports.push(node);
      for (const child of node.children || []) visit(child);
    }
    geometry.children.forEach(visit);
    assert.ok(viewports.length > 0,`Expected stretch viewports: ${tex}`);
    for (const node of viewports) {
      assert.equal(node.attrs.overflow,'hidden');
      assert.equal(node.attrs.preserveAspectRatio,'xMidYMid meet');
      assert.ok(node.children.length > 0);
    }
    assert.ok(validateGeometry(geometry));
  }
});

test('main-side viewport validation rejects hostile attributes and cumulative expansion', () => {
  const attrs = {x:'10',y:'20',width:'100',height:'50',viewBox:'0 0 10 10',overflow:'hidden'};
  const wrap = node => ({viewBox:[0,0,100,100],width:2,height:2,children:[node]});
  const viewport = (changes = {},children = []) => ({tag:'svg',attrs:{...attrs,...changes},children});
  assert.ok(validateGeometry(wrap(viewport())));
  for (const changes of [{width:'0.001'},{width:'NaN'},{height:'100%'},{x:'0x10'},{x:'100001'},{viewBox:'0 0 0 10'},{viewBox:'0 0 1e99 10'},{viewBox:'0 0 1 1 1'},{preserveAspectRatio:'xMinYMid meet'},{overflow:'auto'},{id:'x'},{onload:'evil()'},{href:'x'},{style:'overflow:visible'},{transform:'scale(2)'}]) {
    assert.equal(validateGeometry(wrap(viewport(changes))),false,JSON.stringify(changes));
  }
  const missingWidth = viewport(); delete missingWidth.attrs.width;
  assert.equal(validateGeometry(wrap(missingWidth)),false);
  assert.equal(validateGeometry(wrap(viewport({viewBox:'0 0 1.1.1'}))),false);
  // Each local viewport/transform is allowed; their combined scale is not.
  let node = {tag:'path',attrs:{d:'M0 0L1 1'}};
  for (let i = 0; i < 4; i++) node = viewport({x:'0',y:'0',width:'100',height:'100',viewBox:'0 0 1 1'},[node]);
  assert.equal(validateGeometry(wrap(node)),false);
  assert.equal(validateGeometry(wrap({tag:'g',attrs:{transform:'scale(10000)'},children:[viewport({width:'200'})]})),false);
  assert.equal(validateGeometry(wrap(viewport({width:'1000',height:'1000',viewBox:'0 0 1 1'},[{tag:'g',attrs:{transform:'scale(10000)'},children:[]}]))),false);
  // Meet alignment includes centering and nonzero viewBox origin.
  assert.equal(validateGeometry(wrap({tag:'g',attrs:{transform:'scale(1000)'},children:[viewport({x:'0',y:'0',width:'100',height:'50',viewBox:'-100000 0 10 10'})]})),false);
});

test('main-side validation independently bounds cumulative transforms', () => {
  let node = {tag: 'path', attrs: {d: 'M0 0L1 1'}};
  for (let i = 0; i < 8; i++) node = {tag: 'g', attrs: {transform: 'scale(100)'}, children: [node]};
  assert.equal(validateGeometry({viewBox: [0, 0, 100, 100], width: 1, height: 1, children: [node]}), false);
});
test('bounds parser stacks and repeated column processing before allocation', () => {
  const sources = [
    String.raw`\left(`.repeat(70) + 'x' + String.raw`\right)`.repeat(70),
    String.raw`\begin{array}{` + '*{0}{c}'.repeat(257) + String.raw`c}x\end{array}`,
    String.raw`\begin{xxalignat}{999999999}x\end{xxalignat}`,
    String.raw`\begin{alignat*}{999999999}x\end{alignat*}`,
    String.raw`\begin{xalignat*}{999999999}x\end{xalignat*}`,
  ];
  for (const source of sources) {
    assert.throws(() => renderMath(source));
    assert.ok(validateGeometry(renderMath('x+1')));
  }
});
