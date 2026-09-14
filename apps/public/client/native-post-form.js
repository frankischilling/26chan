export function mountNativePostForm({ source, thread, openQuickReply }) {
  const desktop = document.getElementById('togglePostFormLink');
  const top = document.querySelector('#mpostform .mobilePostFormToggle');
  const table = source?.querySelector('#postForm');
  if (!desktop || !top || !table) return;
  const links = [...document.querySelectorAll('.mobilePostFormToggle')];
  const mobile = matchMedia('(max-width: 480px)');
  function sync() {
    const expanded = mobile.matches ? !table.classList.contains('hideMobile') : source.classList.contains('postFormOpen');
    desktop.firstElementChild.setAttribute('aria-expanded', String(expanded));
    for (const link of links) link.setAttribute('aria-expanded', String(!table.classList.contains('hideMobile')));
  }
  function show(event) {
    event?.preventDefault(); source.classList.add('postFormOpen'); desktop.hidden = true; sync();
  }
  desktop.firstElementChild.addEventListener('click', show);
  for (const link of links) {
    link.addEventListener('click', event => {
      event.preventDefault();
      if (openQuickReply()) return;
      const expanded = table.classList.toggle('hideMobile') === false;
      top.classList.toggle('hidden', !expanded); top.classList.toggle('shown', expanded);
      top.textContent = expanded ? 'Close Post Form' : thread ? 'Post Reply' : 'Start New Thread';
      sync();
      if (link !== top) top.scrollIntoView();
    });
    link.parentElement.hidden = false;
  }
  source.classList.add('nativePostForm'); table.classList.add('hideMobile'); desktop.hidden = false;
  mobile.addEventListener('change', sync); sync();
  if (location.hash === '#reply') show();
}
