// Serializable for page.evaluate; observe one exact real Quick Reply request.
// Read a clone before handing the response to the transport, whose completed
// fetch cleanup aborts its controller and can retire Chromium's CDP body cache.
export function installPostingResponseObserver({ url, thread, comment }) {
  const original = window.fetch;
  let resolve, reject;
  window.ownedPostingResponse = new Promise((done, fail) => { resolve = done; reject = fail; });
  window.fetch = async (...args) => {
    const options = args[1];
    const matches = String(args[0]) === url && options?.method === 'POST'
      && options.body?.get?.('resto') === thread && options.body?.get?.('com') === comment;
    if (!matches) return original(...args);
    window.fetch = original;
    try {
      const response = await original(...args);
      const text = await response.clone().text();
      resolve({ status: response.status, type: response.headers.get('content-type'), text });
      return response;
    } catch (error) {
      reject(error);
      throw error;
    }
  };
}
