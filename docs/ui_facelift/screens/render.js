(function(){
  class DCLogic { constructor(p){ this.props=p||{}; this.state={}; } setState(s){ Object.assign(this.state, typeof s==='function'?s(this.state):s); } }
  window.DCLogic = DCLogic;
  const xdc = document.querySelector('x-dc');
  const sc = document.querySelector('script[data-dc-script]');
  const Component = new Function('DCLogic', sc.textContent + '\nreturn Component;')(DCLogic);
  const c = new Component({});
  const vals = c.renderVals();
  const get = (expr, scope) => {
    expr = expr.trim();
    if (expr === 'true') return true; if (expr === 'false') return false;
    let v = scope; for (const k of expr.split('.')) { if (v == null) return undefined; v = v[k]; } return v;
  };
  const interp = (s, scope) => s.replace(/{{(.*?)}}/g, (_, e) => { const v = get(e, scope); return v == null || v === false ? '' : (typeof v === 'function' ? '' : String(v)); });
  const one = (s, scope) => { const m = s.match(/^\s*{{(.*?)}}\s*$/); return m ? get(m[1], scope) : s; };
  function render(node, scope, out) {
    if (node.nodeType === 3) { out.push(document.createTextNode(interp(node.nodeValue, scope))); return; }
    if (node.nodeType !== 1) return;
    const tag = node.tagName.toLowerCase();
    if (tag === 'helmet') { [...node.childNodes].forEach(n => document.head.appendChild(n.cloneNode(true))); return; }
    if (tag === 'sc-for') { const list = one(node.getAttribute('list'), scope) || []; const as = node.getAttribute('as');
      (Array.isArray(list) ? list : []).forEach((it, i) => { const s2 = Object.assign(Object.create(null), scope, {[as]: it}); node.childNodes.forEach(ch => render(ch, s2, out)); }); return; }
    if (tag === 'sc-if') { if (one(node.getAttribute('value'), scope)) node.childNodes.forEach(ch => render(ch, scope, out)); return; }
    const el = document.createElement(tag);
    for (const a of node.attributes) { if (a.name.startsWith('hint-') || /^on/i.test(a.name)) continue; el.setAttribute(a.name, interp(a.value, scope)); }
    const kids = tag === 'template' ? node.content.childNodes : node.childNodes;
    kids.forEach(ch => { const o = []; render(ch, scope, o); o.forEach(n => el.appendChild(n)); });
    out.push(el);
  }
  const out = []; xdc.childNodes.forEach(ch => render(ch, vals, out));
  const host = document.createElement('div'); out.forEach(n => host.appendChild(n));
  xdc.replaceWith(host);
  window.__done = true;
})();
