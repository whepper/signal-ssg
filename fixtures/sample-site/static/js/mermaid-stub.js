// Fixture stub demonstrating the `has_mermaid` gate: a real site loads its
// diagram renderer here (self-hosted, deferred). Signal only decides
// *whether* a page needs it, from normalized code metadata — never how
// diagrams render.
document.querySelectorAll('pre.mermaid').forEach(function (el) {
  el.setAttribute('data-stub-rendered', 'true');
});
