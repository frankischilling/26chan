import { createQualificationProbe } from './generated/consumer-v1.mjs';
const caseId = new URL(location.href).searchParams.get('case');
window.referenceProbeReady = createQualificationProbe(caseId, 'original-dom');
