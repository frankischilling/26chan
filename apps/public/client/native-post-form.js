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
  function showDesktop() {
    source.classList.add('postFormOpen');
    desktop.hidden = true;
  }
  function showMobile(expanded) {
    table.classList.toggle('hideMobile', !expanded);
    top.classList.toggle('hidden', !expanded);
    top.classList.toggle('shown', expanded);
    top.textContent = expanded ? 'Close Post Form' : thread ? 'Post Reply' : 'Start New Thread';
  }
  function showFromHash() {
    if (location.hash !== '#reply') return;
    showDesktop();
    showMobile(true);
    sync();
  }
  desktop.firstElementChild.addEventListener('click', event => {
    event.preventDefault();
    showDesktop();
    sync();
  });
  for (const link of links) {
    link.addEventListener('click', event => {
      event.preventDefault();
      if (openQuickReply()) return;
      showMobile(table.classList.contains('hideMobile'));
      sync();
      if (link !== top) top.scrollIntoView();
    });
    link.parentElement.hidden = false;
  }
  source.classList.add('nativePostForm'); table.classList.add('hideMobile'); desktop.hidden = false;
  mobile.addEventListener('change', sync); sync();
  window.addEventListener('hashchange', showFromHash);
  showFromHash();
}
