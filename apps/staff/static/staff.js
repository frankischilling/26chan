'use strict';
const status = document.getElementById('status');
async function post(path, data) {
  const response = await fetch(path, { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(data) });
  if (!response.ok) throw new Error('Request rejected');
  return response.json();
}
for (const kind of ['enroll', 'login']) {
  document.getElementById(kind).addEventListener('submit', async event => {
    event.preventDefault();
    const form = event.currentTarget;
    const button = form.querySelector('button');
    button.disabled = true;
    status.textContent = 'Waiting for your passkey…';
    try {
      const data = Object.fromEntries(new FormData(form));
      const challenge = await post(`/${kind}/start`, data);
      let credential;
      if (kind === 'enroll') {
        const publicKey = PublicKeyCredential.parseCreationOptionsFromJSON(challenge.publicKey);
        credential = await navigator.credentials.create({ publicKey });
      } else {
        const publicKey = PublicKeyCredential.parseRequestOptionsFromJSON(challenge.publicKey);
        credential = await navigator.credentials.get({ publicKey });
      }
      await post(`/${kind}/finish`, credential.toJSON());
      if (kind === 'login') window.location.assign('/reports');
      else { form.reset(); status.textContent = 'Passkey enrolled. Sign in with your account.'; }
    } catch {
      status.textContent = 'Request could not be completed. Start again or contact your operator.';
    } finally { button.disabled = false; }
  });
}
