// Run exact geometry statements from the supplied source with owned metadata.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const fixturePath = path.join(root, 'fixtures/swf-embed-reference.json');
const check = process.argv.includes('--check');
const sourcePath = process.argv.slice(2).find(value => value !== '--check');
let thread, index;
if (sourcePath) {
  const raw = fs.readFileSync(path.join(sourcePath, 'js/extension.js'));
  if (crypto.createHash('sha256').update(raw).digest('hex') !== '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31') throw new Error('Reference source changed');
  const source = raw.toString('utf8').replaceAll('\r\n', '\n');
  const start = source.indexOf('  maxWidth = document.documentElement.clientWidth - (!Main.hasMobileLayout ? 100 : 30);');
  thread = source.slice(start, source.indexOf("  cnt = document.createElement('div');", start)).trim();
  const startIndex = source.indexOf('  cntWidth = width = +el.getAttribute(\'data-width\');', source.indexOf('SWFEmbed.embedIndex ='));
  index = source.slice(startIndex, source.indexOf('  el = SWFEmbed.getFrameNode(', startIndex)).trim();
} else {
  if (!check) throw new Error('source tree required for initial extraction');
  ({ thread, index } = JSON.parse(fs.readFileSync(fixturePath, 'utf8')));
}
const hash = value => crypto.createHash('sha256').update(value).digest('hex');
if (hash(thread) !== 'cc6561a1256381518b86b373bc6e981bfb38cb885140c9a933e4bccc10eda975' || hash(index) !== '9f08db06836997d0858a377d1936b86567d822b9ba43078f7ce096d306084f8e') throw new Error('Source geometry slice changed');
const cases = [];
for (const [width, height] of [[640,480],[1920,1080],[480,1920],[31,17],[10001,1]]) {
  for (const [viewportWidth, viewportHeight] of [[1280,900],[390,844],[800,600]]) {
    for (const mobile of [false,true]) {
      const element = {getAttribute: name => name === 'data-width' ? width : height};
      const context = {link:element,el:element,Main:{hasMobileLayout:mobile},document:{documentElement:{clientWidth:viewportWidth,clientHeight:viewportHeight}},margins:10,headerHeight:20};
      vm.runInNewContext(thread, context, {timeout:100});
      const threadResult = {width:context.width,height:context.height};
      vm.runInNewContext(index, context, {timeout:100});
      cases.push({width,height,viewportWidth,viewportHeight,mobile,thread:threadResult,index:{width:context.cntWidth,height:context.cntHeight}});
    }
  }
}
const encoded=JSON.stringify({thread,index,thread_sha256:hash(thread),index_sha256:hash(index),cases},null,2)+'\n';
if(check){if(fs.readFileSync(fixturePath,'utf8')!==encoded)throw new Error('source embed fixture differs');}
else fs.writeFileSync(fixturePath,encoded);
console.log(`Checked ${cases.length} source embed geometry cases.`);
