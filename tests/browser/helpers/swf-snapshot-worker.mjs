// Node adapter for the release worker's message entry point. No DOM/browser.
import {parentPort} from 'node:worker_threads';
globalThis.WorkerGlobalScope=class {static [Symbol.hasInstance](value){return value===globalThis;}};
globalThis.addEventListener=(name,handler)=>{if(name==='message')parentPort.on('message',data=>handler({data}));};
globalThis.postMessage=value=>parentPort.postMessage(value);
await import('../../../apps/public/static/native-filter.v1.js');
parentPort.postMessage({ready:true});
