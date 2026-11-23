// Target-owned navigation for native Mintlify disclosures and fragment aliases. Authored content
// supplies neither code nor selectors. The native details/toggle handler keeps
// state and accessibility attributes in sync; heading IDs and URLs stay intact.
(() => {
  let scheduled = false;

  function reveal() {
    scheduled = false;
    let id;
    try {
      id = decodeURIComponent(window.location.hash.slice(1));
    } catch {
      return;
    }
    if (!id) return;
    const target = document.getElementById(id) ||
      [...document.getElementsByName(id)].find((element) => element instanceof HTMLAnchorElement);
    if (!target || !target.closest('#content')) return;

    const closed = [];
    for (let ancestor = target.parentElement; ancestor; ancestor = ancestor.parentElement) {
      if (ancestor instanceof HTMLDetailsElement &&
          ancestor.classList.contains('gazette-layout-disclosure') && !ancestor.open) {
        closed.push(ancestor);
      }
    }
    for (const disclosure of closed.reverse()) disclosure.open = true;
    const fragmentAnchor = target instanceof HTMLAnchorElement && !target.hasAttribute('href');
    let titleOffset = 0;
    // The transport retains a stripped title, and its earlier names, as the
    // opening anchors. Keep the visible page title with that document entrance.
    if (fragmentAnchor && !target.childNodes.length && target.parentElement.id === 'content') {
      let previous = target.previousElementSibling;
      while (previous?.matches('a[id]:not([href]):empty, a[name]:not([href]):empty')) {
        previous = previous.previousElementSibling;
      }
      const title = !previous && document.getElementById('page-title');
      if (title) {
        titleOffset = Math.max(0, target.getBoundingClientRect().top - title.getBoundingClientRect().top);
      }
    }
    if (fragmentAnchor) target.style.setProperty('--gazette-title-fragment-offset', `${titleOffset}px`);
    if (closed.length || fragmentAnchor) {
      requestAnimationFrame(() => target.scrollIntoView({ block: 'start' }));
    }
  }

  function schedule() {
    if (!scheduled) {
      scheduled = true;
      requestAnimationFrame(reveal);
    }
  }

  // The documented provider hook runs after client-side page replacement.
  window.addEventListener('mintlify:navigate', schedule);
  window.addEventListener('hashchange', schedule);
  window.addEventListener('popstate', schedule);
  document.addEventListener('click', (event) => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const link = event.target instanceof Element ? event.target.closest('a[href]') : null;
    if (!link || link.hasAttribute('download') || (link.target && link.target !== '_self')) return;
    const destination = new URL(link.href, window.location.href);
    if (destination.origin === window.location.origin &&
        destination.pathname === window.location.pathname &&
        destination.search === window.location.search && destination.hash) {
      schedule();
    }
  }, true);
  schedule();
})();
