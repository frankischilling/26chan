import { withSourceCaptchaFixture } from './deletion-quota-fixture.js';
export function sourceCaptchaTest(base, boards) {
  return base.extend({
    ownedSourceCaptcha: [async ({}, use) => {
      await withSourceCaptchaFixture(boards, () => use());
    }, { auto: true }],
  });
}
