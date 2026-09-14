import { parentPort } from 'node:worker_threads';
import { runNativeFilterJob } from '../../../apps/public/static/native-filter.v1.js';

parentPort.on('message', raw => parentPort.postMessage(runNativeFilterJob(raw)));
