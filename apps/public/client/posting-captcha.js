import { mountPostingCaptcha } from './posting-captcha-core.js';
export { createPostingCaptcha, mountPostingCaptcha, validReply, validScope } from './posting-captcha-core.js';
if (typeof document !== 'undefined') {
  for (const root of document.querySelectorAll('[data-posting-captcha]')) mountPostingCaptcha(root);
}
