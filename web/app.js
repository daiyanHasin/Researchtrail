/* ResearchTrail client. Plain JavaScript, no build step: talks to the local API
   and redraws when the project changes (a paper arrives, a file is saved). */
(() => {
  'use strict';
  const $ = (s, el = document) => el.querySelector(s);
  const $$ = (s, el = document) => [...el.querySelectorAll(s)];
  const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const plural = (n, w, p) => `${n} ${n === 1 ? w : (p || w + 's')}`;
  const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;

  const S = { open: null, version: 0, view: 'overview', arg: '', state: null, lastCoverage: {}, jobsSeen: {} };

  async function api(path, opts = {}) {
    const init = { method: opts.method || 'GET', headers: {} };
    if (opts.body !== undefined) {
      if (opts.body instanceof Blob || opts.body instanceof ArrayBuffer) init.body = opts.body;
      else { init.body = JSON.stringify(opts.body); init.headers['Content-Type'] = 'application/json'; }
    }
    const r = await fetch(path, init);
    const data = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(data.error || `Request failed (${r.status})`);
    return data;
  }

  function toast(msg, kind = 'info', action) {
    const el = document.createElement('div');
    const tone = kind === 'error' ? 'border-bad/40' : kind === 'ok' ? 'border-accent/40' : 'border-line';
    el.className = `toast-in pointer-events-auto panel-raised flex items-start gap-3 border ${tone} px-4 py-3 text-[14px]`;
    el.innerHTML = `<div class="flex-1">${esc(msg)}</div>${action ? `<a class="font-medium text-accent hover:underline" href="${action.href}">${esc(action.label)}</a>` : ''}<button class="text-ink3 hover:text-ink" aria-label="Dismiss">✕</button>`;
    el.querySelector('button').onclick = () => el.remove();
    $('#toasts').appendChild(el);
    setTimeout(() => el.remove(), kind === 'error' ? 9000 : 5500);
  }

  function ago(t) {
    if (!t) return '';
    const s = Math.max(0, Date.now() / 1000 - t);
    if (s < 60) return 'just now';
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    if (s < 172800) return 'yesterday';
    return new Date(t * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: s > 31536000 ? 'numeric' : undefined });
  }

  const KIND = { arxiv: 'arXiv', doi: 'DOI', journal: 'Journal', github: 'GitHub', web: 'Web page', pdf: 'PDF', 'pdf-upload': 'Uploaded PDF', demo: 'Demo example' };
  const SOURCE = { rules: 'Found by rules in the paper text', chat: 'Added from your chat and checked against its passage', model: 'Added by your local model and checked against its passage', you: 'Added by you', code: 'Found in your code' };

  function setTitle(t, sub = '') { $('#viewTitle').textContent = t; $('#viewSub').textContent = sub; document.title = `${t} · ResearchTrail`; }
  function main(html) { const m = $('#main'); m.innerHTML = `<div class="view-enter">${html}</div>`; return m; }
  function empty(title, body, actions = '') {
    return `<div class="panel mx-auto max-w-[620px] px-8 py-10 text-center"><h2 class="mb-2 text-[18px] font-semibold">${esc(title)}</h2><p class="mx-auto mb-5 max-w-[46ch] text-ink2">${body}</p>${actions}</div>`;
  }

  // ---------------- drawer ----------------
  function drawer(title, html, onMount) {
    const d = $('#drawer');
    $('#drawerTitle').textContent = title;
    $('#drawerBody').innerHTML = html;
    d.hidden = false;
    $$('[data-close]', d).forEach((b) => (b.onclick = closeDrawer));
    if (onMount) onMount($('#drawerBody'));
    setTimeout(() => $('button[data-close]', d)?.focus(), 30);
  }
  function closeDrawer() { $('#drawer').hidden = true; }
  addEventListener('keydown', (e) => { if (e.key === 'Escape') closeDrawer(); });

  // ---------------- live updates ----------------
  async function pulse() {
    try {
      const p = await api('/api/pulse');
      setLive(true);
      if (p.open !== S.open) { S.open = p.open; S.version = p.version; await boot(); }
      else if (p.open) {
        renderJobs(p.jobs || []);
        if (p.version !== S.version) { S.version = p.version; refresh(); }
      }
    } catch (e) { setLive(false); }
    setTimeout(pulse, 1500);
  }
  function setLive(ok) {
    $('#live').innerHTML = ok ? '<span class="dot !h-2 !w-2 bg-accent"></span>Live' : '<span class="dot !h-2 !w-2 bg-bad"></span>Not running';
    $('#live').title = ok ? 'Updates as soon as you save a file or a paper arrives' : 'Start ResearchTrail again with start.bat or start.sh';
  }
  let refreshTimer;
  function refresh() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => {
      const a = document.activeElement;
      const typing = a && $('#main').contains(a) && /INPUT|TEXTAREA|SELECT/.test(a.tagName);
      if (!typing && $('#drawer').hidden) route(true);
    }, 250);
  }

  function renderJobs(jobs) {
    const box = $('#jobs');
    box.innerHTML = jobs.filter((j) => !j.finished).map((j) => `<div class="working border-t border-line px-6 py-2 text-[13px] text-ink2"><span class="font-medium text-ink">${esc(j.input.length > 70 ? j.input.slice(0, 70) + '…' : j.input)}</span>: ${esc(j.message)}</div>`).join('');
    for (const j of jobs) {
      if (!j.finished || S.jobsSeen[j.id]) continue;
      S.jobsSeen[j.id] = true;
      if (j.status === 'done') toast(j.message, 'ok', j.card ? { href: `#/papers/${j.card}`, label: 'Open card' } : null);
      else toast(j.message, 'error', j.card ? { href: `#/papers/${j.card}`, label: 'Show it' } : null);
    }
  }

  // ---------------- boot & routing ----------------
  async function boot() {
    if (!S.open) { $('#shell').hidden = true; $('#welcome').hidden = false; renderWelcome(); return; }
    $('#welcome').hidden = true; $('#shell').hidden = false;
    route();
  }

  addEventListener('hashchange', () => { closeDrawer(); route(); });
  async function route(isRefresh = false) {
    if (!S.open) return;
    const [, view = 'overview', arg = ''] = (location.hash || '#/overview').split('/');
    const sameView = view === S.view && decodeURIComponent(arg) === S.arg;
    S.view = view; S.arg = decodeURIComponent(arg);
    $$('[data-view]').forEach((a) => { if (a.dataset.view === view) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current'); });
    try {
      S.state = await api('/api/state');
      $('#projectName').textContent = S.state.project.name;
      $('#paperCount').textContent = S.state.counts.papers || '';
      if (isRefresh && sameView && ['ask', 'packs', 'settings'].includes(view)) return;
      const views = { overview: viewOverview, papers: viewPapers, ask: viewAsk, compare: viewCompare, code: viewCode, packs: viewPacks, settings: viewSettings };
      await (views[view] || viewOverview)(isRefresh && sameView);
    } catch (e) { toast(e.message, 'error'); }
  }

  // ---------------- overview ----------------
  async function viewOverview(isRefresh) {
    const st = S.state;
    const c = st.counts;
    setTitle('Overview', c.papers || c.versions ? `${plural(c.papers, 'paper')} in your library and ${plural(c.versions, 'saved version')} of your code` : 'Add a paper or save a file to start your trail');
    const total = Math.max(1, c.papers);
    const cov = st.coverage;
    const W = 1000, H = 262;
    const xs = cov.map((_, i) => 90 + i * ((W - 180) / (cov.length - 1)));
    const ys = cov.map((_, i) => (i % 2 ? 100 : 152));
    let d = `M ${xs[0] - 70} ${ys[0]} L ${xs[0]} ${ys[0]}`;
    for (let i = 1; i < xs.length; i++) { const mx = (xs[i - 1] + xs[i]) / 2; d += ` C ${mx} ${ys[i - 1]}, ${mx} ${ys[i]}, ${xs[i]} ${ys[i]}`; }
    d += ` L ${xs.at(-1) + 70} ${ys.at(-1)}`;
    const R = 34, r = 24, CR = 2 * Math.PI * R, Cr = 2 * Math.PI * r;
    const changed = new Set(cov.filter((s) => S.lastCoverage[s.stage] && S.lastCoverage[s.stage] !== `${s.papers}/${s.mine}`).map((s) => s.stage));
    cov.forEach((s) => (S.lastCoverage[s.stage] = `${s.papers}/${s.mine}`));
    const stations = cov.map((s, i) => {
      const fp = s.papers / total, fm = s.paper_techniques ? Math.min(1, s.mine / s.paper_techniques) : (s.mine ? 1 : 0);
      const delay = reduced || isRefresh ? 0 : 0.35 + i * 0.12;
      const below = ys[i] > 120;
      return `<a href="#/compare/${s.stage}" class="${isRefresh ? '' : 'station'} group" style="animation-delay:${delay}s" aria-label="${esc(s.name)}: ${s.papers} of ${c.papers} papers, ${s.mine} techniques in your code">
        <g transform="translate(${xs[i]} ${ys[i]})">
          ${changed.has(s.stage) && !reduced ? `<circle r="${R + 6}" fill="none" stroke="rgb(var(--accent))" stroke-width="2" class="pulse-ring"/>` : ''}
          <circle r="${R + 10}" fill="rgb(var(--raised))"/>
          <circle r="${R}" fill="rgb(var(--raised))" stroke="rgb(var(--line))" stroke-width="7"/>
          <circle r="${R}" fill="none" stroke="rgb(var(--accent))" stroke-width="7" stroke-linecap="round" transform="rotate(-90)" class="arc" stroke-dasharray="${CR}" stroke-dashoffset="${isRefresh ? CR * (1 - fp) : CR}" data-to="${CR * (1 - fp)}"/>
          <circle r="${r}" fill="none" stroke="rgb(var(--line))" stroke-width="5" opacity=".7"/>
          <circle r="${r}" fill="none" stroke="rgb(var(--mine))" stroke-width="5" stroke-linecap="round" transform="rotate(-90)" class="arc" stroke-dasharray="${Cr}" stroke-dashoffset="${isRefresh ? Cr * (1 - fm) : Cr}" data-to="${Cr * (1 - fm)}"/>
          <text y="6" text-anchor="middle" font-size="17" font-weight="600" fill="rgb(var(--ink))">${s.shared}</text>
          <g transform="translate(0 ${below ? 66 : -72})">
            <text text-anchor="middle" font-size="15" font-weight="600" fill="rgb(var(--ink))" class="group-hover:underline">${esc(s.name)}</text>
            <text y="18" text-anchor="middle" font-size="12.5" fill="rgb(var(--ink3))">${s.papers} of ${c.papers} papers, ${s.mine} yours</text>
          </g>
        </g></a>`;
    }).join('');
    const nothing = !c.papers && !c.files;
    main(`
      <section class="panel-raised mb-6 overflow-hidden px-4 pb-2 pt-5">
        <div class="flex flex-wrap items-baseline justify-between gap-2 px-3">
          <h2 class="text-[16px] font-semibold">Your research trail</h2>
          <p class="text-[13px] text-ink3"><span class="mr-1 inline-block h-2.5 w-2.5 rounded-full bg-accent align-middle"></span>Outer ring: papers covering the stage <span class="ml-3 mr-1 inline-block h-2.5 w-2.5 rounded-full bg-mine align-middle"></span>Inner ring: your code</p>
        </div>
        <svg viewBox="0 0 ${W} ${H}" class="mt-1 w-full" role="img" aria-label="Pipeline stages from data to evaluation">
          <path d="${d}" fill="none" stroke="rgb(var(--accent) / .35)" stroke-width="3" stroke-linecap="round" class="${isRefresh ? '' : 'trail-path'}"/>
          ${stations}
        </svg>
        <p class="px-3 pb-3 text-[13px] text-ink3">The number in each ring counts techniques that both your papers and your code use. Select a stage to compare it side by side.</p>
      </section>
      ${nothing ? empty('Your trail starts here', 'Paste a link to a paper you\'re reading in the box above. Save any Python file or notebook in this folder and its versions appear here too.', `<button class="btn" id="demoHint">Show me with the demo project</button>`) : `
      <div class="grid gap-6 lg:grid-cols-[1.1fr_1fr]">
        <section class="panel-raised p-5"><h2 class="mb-1 text-[16px] font-semibold">How your library grew</h2><p class="mb-3 text-[13px] text-ink3">Papers added and code versions saved, over time.</p>${growthChart(st.growth)}</section>
        <section class="panel-raised p-5"><h2 class="mb-3 text-[16px] font-semibold">Recent activity</h2><ol class="scroll-thin max-h-[330px] space-y-3 overflow-y-auto pr-2">${st.timeline.map(tl).join('') || '<li class="text-ink3">Nothing yet.</li>'}</ol></section>
      </div>`}`);
    $('#demoHint')?.addEventListener('click', openDemo);
    if (!isRefresh) setTimeout(() => $$('.arc').forEach((a) => a.setAttribute('stroke-dashoffset', a.dataset.to)), reduced ? 0 : 450);
  }

  function tl(e) {
    const icon = e.kind === 'paper' ? 'bg-accent' : e.kind === 'version' ? 'bg-mine' : e.kind === 'restore' ? 'bg-warn' : 'bg-ink3';
    const href = e.kind === 'paper' && e.link ? `#/papers/${e.link}` : e.kind === 'version' ? '#/code' : null;
    return `<li class="flex gap-3"><span class="dot mt-1.5 !h-2.5 !w-2.5 flex-none ${icon}"></span><div class="min-w-0"><div class="text-[14px] leading-snug">${href ? `<a class="hover:underline" href="${href}">${esc(e.text)}</a>` : esc(e.text)}</div><div class="text-[12px] text-ink3">${ago(e.time)}</div></div></li>`;
  }

  function growthChart(g) {
    if (!g.length) return '<p class="text-ink3">Nothing to chart yet.</p>';
    const W = 560, H = 180, pad = 28;
    const t0 = g[0].t, t1 = Math.max(g.at(-1).t, t0 + 3600);
    const maxP = Math.max(1, ...g.map((x) => x.papers)), maxV = Math.max(1, ...g.map((x) => x.versions));
    const X = (t) => pad + ((t - t0) / (t1 - t0)) * (W - pad * 2);
    const Y = (v, max) => H - pad - (v / max) * (H - pad * 2);
    const line = (key, max) => { let p = ''; let prev = 0; g.forEach((x, i) => { p += i ? ` L ${X(x.t)} ${Y(prev, max)} L ${X(x.t)} ${Y(x[key], max)}` : `M ${X(x.t)} ${Y(x[key], max)}`; prev = x[key]; }); return p + ` L ${W - pad} ${Y(prev, max)}`; };
    const last = g.at(-1);
    return `<svg viewBox="0 0 ${W} ${H}" class="w-full" role="img" aria-label="Library growth: ${last.papers} papers and ${last.versions} code versions">
      <line x1="${pad}" x2="${W - pad}" y1="${H - pad}" y2="${H - pad}" stroke="rgb(var(--line))"/>
      <path d="${line('versions', maxV)}" fill="none" stroke="rgb(var(--mine))" stroke-width="2.2" stroke-linejoin="round"/>
      <path d="${line('papers', maxP)}" fill="none" stroke="rgb(var(--accent))" stroke-width="2.6" stroke-linejoin="round"/>
      <text x="${pad}" y="${H - 8}" font-size="11.5" fill="rgb(var(--ink3))">${esc(new Date(t0 * 1000).toLocaleDateString())}</text>
      <text x="${W - pad}" y="${H - 8}" font-size="11.5" text-anchor="end" fill="rgb(var(--ink3))">now</text>
      <text x="${pad + 4}" y="18" font-size="12.5" fill="rgb(var(--accent))" font-weight="600">${plural(last.papers, 'paper')}</text>
      <text x="${pad + 4}" y="36" font-size="12.5" fill="rgb(var(--mine))" font-weight="600">${plural(last.versions, 'code version')}</text>
    </svg>`;
  }

  // ---------------- papers ----------------
  async function viewPapers() {
    const papers = await api('/api/papers');
    const id = S.arg || papers[0]?.id;
    setTitle('Papers', papers.length ? `${plural(papers.length, 'paper')}. Each was read once; only its card is kept.` : '');
    if (!papers.length) {
      main(empty('No papers yet', 'Paste a link above: an arXiv page, a DOI, a journal article, a blog post or a GitHub repo. Only have a PDF? Upload it; it\'s read and then deleted.', uploadButton()));
      wireUpload(); return;
    }
    const scrollList = $('#paperList')?.scrollTop || 0;
    main(`<div class="grid gap-6 lg:grid-cols-[340px_1fr]">
      <div class="flex flex-col gap-3">
        <input id="paperFilter" class="input" placeholder="Filter by title, author or technique" value="${esc(S.paperFilter || '')}" aria-label="Filter papers">
        <ul id="paperList" class="scroll-thin flex max-h-[calc(100vh-230px)] flex-col gap-2 overflow-y-auto pr-1">${papers.map((p) => paperItem(p, p.id === id)).join('')}</ul>
        ${uploadButton()}
      </div>
      <div id="paperDetail"></div>
    </div>`);
    $('#paperList').scrollTop = scrollList;
    wireUpload();
    const f = $('#paperFilter');
    const apply = () => { S.paperFilter = f.value; const q = f.value.toLowerCase(); $$('#paperList li').forEach((li) => (li.hidden = !!q && !li.dataset.search.includes(q))); };
    f.oninput = apply; apply();
    if (id) await paperDetail(id);
  }
  function uploadButton() { return `<label class="btn btn-quiet cursor-pointer justify-start"><input id="pdfInput" type="file" accept="application/pdf" class="sr-only">Upload a PDF instead</label>`; }
  function wireUpload() {
    const i = $('#pdfInput'); if (!i) return;
    i.onchange = async () => {
      const f = i.files[0]; if (!f) return;
      try { await api(`/api/papers/pdf?name=${encodeURIComponent(f.name)}`, { method: 'POST', body: f }); toast(`Reading ${f.name}. The PDF won't be kept.`); } catch (e) { toast(e.message, 'error'); }
      i.value = '';
    };
  }
  function paperItem(p, active) {
    const search = [p.title, p.cite, ...(p.authors || []), ...Object.values(p.stages).flat()].join(' ').toLowerCase();
    return `<li data-search="${esc(search)}"><a href="#/papers/${p.id}" class="block rounded-card border px-4 py-3 transition ${active ? 'border-accent/50 bg-raised shadow-soft' : 'border-line bg-surface hover:bg-raised'}">
      <div class="font-serif text-[15px] leading-snug text-ink">${esc(p.title)}</div>
      <div class="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-[12.5px] text-ink3"><span>${esc(p.cite)}</span><span>${esc(KIND[p.kind] || p.kind)}</span><span>${plural(p.techniques, 'technique')}</span>${p.unverified ? `<span class="text-warn">${p.unverified} unchecked</span>` : ''}</div>
    </a></li>`;
  }

  async function paperDetail(id) {
    const box = $('#paperDetail');
    let c;
    try { c = await api(`/api/papers/${encodeURIComponent(id)}`); } catch (e) { box.innerHTML = empty('Paper not found', esc(e.message)); return; }
    const stageName = Object.fromEntries(c.stage_names.map((s) => [s.id, s.name]));
    const passage = (pid) => c.passages.find((p) => p.id === pid);
    const stages = c.stage_names.filter((s) => (c.stages[s.id] || []).length).map((s) => `
      <div class="border-t border-line py-3 first:border-t-0"><div class="label mb-2">${esc(s.name)}</div><div class="flex flex-wrap gap-2">
      ${c.stages[s.id].map((e) => `<button class="chip ${e.source === 'rules' ? 'chip-accent' : ''} hover:border-accent" data-entry="${esc(s.id)}|${esc(e.technique)}" title="${esc(SOURCE[e.source] || e.source)}">${esc(c.technique_names[e.technique] || e.name)}${e.source !== 'rules' ? ' <span class="text-accent">✓</span>' : ''}</button>`).join('')}
      </div></div>`).join('');
    const facts = (title, list, fmt) => list.length ? `<div><div class="label mb-1.5">${title}</div><ul class="space-y-1 text-[14px]">${list.slice(0, 8).map((f) => `<li><button class="text-left hover:underline" data-passage="${esc(f.passage)}">${fmt(f)}</button></li>`).join('')}</ul></div>` : '';
    box.innerHTML = `<article class="panel-raised view-enter p-6">
      <div class="mb-1 flex flex-wrap items-center gap-2 text-[12.5px] text-ink3"><span class="chip">${esc(KIND[c.kind] || c.kind)}</span>${c.canonical ? `<span>${esc(c.canonical)}</span>` : ''}<span>added ${ago(c.added)}</span></div>
      <h2 class="mb-2 max-w-[60ch] font-serif text-[25px] leading-tight">${esc(c.title)}</h2>
      <p class="mb-4 text-[14px] text-ink2">${esc((c.authors || []).join(', '))}${c.year ? ` (${c.year})` : ''}</p>
      <div class="mb-5 flex flex-wrap gap-2">
        ${c.link ? `<a class="btn" href="${esc(c.link)}" target="_blank" rel="noopener">Open the original</a>` : ''}
        ${c.archive ? `<a class="btn btn-quiet" href="${esc(c.archive)}" target="_blank" rel="noopener">Archived copy</a>` : ''}
        <button class="btn btn-quiet" id="refetch" title="Read the link again with the current extractor">Re-fetch</button>
        <button class="btn btn-quiet btn-danger" id="removePaper">Remove from library</button>
      </div>
      ${c.summary ? `<p class="paper-text mb-5 max-w-[68ch]">${esc(c.summary)}</p>` : ''}
      ${(c.reader_notes || []).map((n) => `<p class="mb-2 max-w-[72ch] rounded-lg bg-sunken px-3 py-2 text-[13px] text-ink2">${esc(n)}</p>`).join('')}
      <section class="mt-5"><h3 class="mb-1 text-[15px] font-semibold">What this paper did</h3><p class="mb-2 text-[13px] text-ink3">Select a technique to see the sentence it came from, or to remove it if it's wrong.</p>
        ${stages || '<p class="text-ink3">No techniques recognised yet. Improve the card below with your chat or a local model.</p>'}</section>
      <section class="mt-5 grid gap-5 sm:grid-cols-3">
        ${facts('Datasets', c.datasets, (f) => esc(f.label))}
        ${facts('Reported results', c.metrics, (f) => `${esc(f.label)} <span class="font-semibold">${esc(f.value)}</span>`)}
        ${facts('Settings', c.hyperparams, (f) => `${esc(f.label)} <span class="font-semibold">${esc(f.value)}</span>`)}
      </section>
      ${c.related?.length ? `<section class="mt-5 rounded-lg border border-line px-4 py-3"><div class="label mb-1">This README links</div>${c.related.map((r) => `<button class="chip mr-2 hover:border-accent" data-add="${esc(r)}">Add ${esc(r)}</button>`).join('')}</section>` : ''}
      <section class="mt-6 rounded-card border border-line bg-surface p-4">
        <h3 class="text-[15px] font-semibold">Improve this card</h3>
        <p class="mb-3 mt-1 max-w-[70ch] text-[13.5px] text-ink2">Rules found what they could. A chat or a local model can find more. Every answer must point to a passage, and anything its passage doesn't say is set aside as unchecked.</p>
        <div class="flex flex-wrap gap-2"><button class="btn" id="copyPrompt">1. Copy the prompt</button>${S.state.config.ollama ? '<button class="btn" id="localModel">Ask my local model</button>' : ''}</div>
        <label class="label mb-1 mt-3 block" for="answer">2. Paste the chat's JSON answer</label>
        <textarea id="answer" rows="3" class="input font-mono text-[12.5px]" placeholder='{"stages": {"features": [{"technique": "encode_frequency", "how": "…", "passage": "k4"}]}}'></textarea>
        <button class="btn btn-primary mt-2" id="checkAnswer">Check and add</button>
        <div id="answerReport" class="mt-2 text-[13.5px]"></div>
        ${c.unverified?.length ? `<details class="mt-3 text-[13px] text-ink2"><summary class="cursor-pointer text-warn">${plural(c.unverified.length, 'item')} set aside as unchecked</summary><ul class="ml-4 mt-1 list-disc">${c.unverified.map((u) => `<li>${esc(u)}</li>`).join('')}</ul></details>` : ''}
      </section>
      <section class="mt-6"><label class="mb-1 block text-[15px] font-semibold" for="note">Your notes</label>
        <textarea id="note" rows="4" class="input paper-text" placeholder="What you think of this paper, what to try from it…">${esc(c.note || '')}</textarea>
        <p class="mt-1 text-[12px] text-ink3">Saved as Markdown in .trail/notes when you leave the box.</p></section>
      <details class="mt-6"><summary class="cursor-pointer text-[15px] font-semibold">Key passages kept (${c.passages.length})</summary>
        <ol class="mt-3 space-y-3">${c.passages.map((p) => `<li class="rounded-lg border border-line bg-surface px-4 py-3"><div class="mb-1 text-[12px] text-ink3">${esc(p.id)}, ${esc(p.section)}, ${esc(stageName[p.stage] || 'context')}</div><p class="paper-text">${esc(p.text)}</p></li>`).join('')}</ol>
        <p class="mt-2 text-[12px] text-ink3">Kept from a source of ${c.source_words.toLocaleString()} words. Fingerprint ${esc(c.fingerprint.slice(0, 19))}…</p></details>
    </article>`;
    const showPassages = (title, pids, extra = '') => drawer(title, pids.map(passage).filter(Boolean).map((p) => `<figure class="mb-4"><figcaption class="mb-1 text-[12px] text-ink3">${esc(p.id)}, ${esc(p.section)}</figcaption><blockquote class="paper-text border-l-2 border-accent pl-3">${esc(p.text)}</blockquote></figure>`).join('') + extra);
    $$('[data-entry]', box).forEach((b) => b.onclick = () => {
      const [stage, tech] = b.dataset.entry.split('|');
      const e = c.stages[stage].find((x) => x.technique === tech);
      showPassages(c.technique_names[tech] || e.name, e.passages, `<p class="mb-3 text-[13px] text-ink3">${esc(SOURCE[e.source] || e.source)}</p><button class="btn btn-danger" id="dropEntry">This is wrong: remove it from the card</button>`);
      $('#dropEntry').onclick = async () => { await api(`/api/papers/${c.id}/entry`, { method: 'POST', body: { stage, technique: tech } }); closeDrawer(); toast('Removed from the card.'); paperDetail(c.id); };
    });
    $$('[data-passage]', box).forEach((b) => b.onclick = () => showPassages('Source passage', [b.dataset.passage]));
    $$('[data-add]', box).forEach((b) => b.onclick = () => addLink(b.dataset.add));
    $('#refetch').onclick = async () => { try { await api(`/api/papers/${c.id}/refetch`, { method: 'POST' }); toast('Reading the link again…'); } catch (e) { toast(e.message, 'error'); } };
    $('#removePaper').onclick = async () => { if (!confirm(`Remove "${c.title}" and its card from your library?`)) return; await api(`/api/papers/${c.id}`, { method: 'DELETE' }); location.hash = '#/papers'; toast('Removed.'); };
    $('#copyPrompt').onclick = async () => { const p = await api(`/api/papers/${c.id}/prompt`); await copy(p.prompt); toast(`Prompt copied, about ${p.tokens.toLocaleString()} tokens. Paste it into any chat, then paste its JSON answer here.`, 'ok'); };
    $('#localModel')?.addEventListener('click', async () => { try { await api(`/api/papers/${c.id}/local`, { method: 'POST' }); } catch (e) { toast(e.message, 'error'); } });
    $('#checkAnswer').onclick = async () => {
      try {
        const r = await api(`/api/papers/${c.id}/answer`, { method: 'POST', body: { answer: $('#answer').value } });
        $('#answer').value = '';
        await paperDetail(c.id);
        $('#answerReport').innerHTML = `<span class="text-good">Added ${plural(r.added, 'checked item')}.</span> ${r.already ? `${r.already} already on the card.` : ''} ${r.unverified.length ? `<span class="text-warn">${plural(r.unverified.length, 'item')} didn't match their passage and ${r.unverified.length === 1 ? 'was' : 'were'} set aside.</span>` : ''}`;
      } catch (e) { $('#answerReport').innerHTML = `<span class="text-bad">${esc(e.message)}</span>`; }
    };
    $('#note').onblur = async (e) => { await api(`/api/papers/${c.id}/note`, { method: 'POST', body: { note: e.target.value } }); };
  }

  async function copy(text) {
    try { await navigator.clipboard.writeText(text); }
    catch { const t = document.createElement('textarea'); t.value = text; document.body.appendChild(t); t.select(); document.execCommand('copy'); t.remove(); }
  }

  // ---------------- ask ----------------
  async function viewAsk() {
    setTitle('Ask your papers', 'Searches the passages kept from every paper. Every result shows where it came from.');
    const stages = S.state.stages;
    main(`<div class="mx-auto max-w-[860px]">
      <form id="askForm" class="panel-raised mb-5 p-4">
        <label for="askQ" class="sr-only">Question</label>
        <input id="askQ" class="input !py-3 !text-[16px]" placeholder="Which papers handle class imbalance, and how?" value="${esc(S.askQ || '')}">
        <div class="mt-3 flex flex-wrap items-center gap-3"><div class="seg flex-wrap" id="askStage"><button type="button" data-s="" aria-pressed="${!S.askStage}">All stages</button>${stages.map((s) => `<button type="button" data-s="${s.id}" aria-pressed="${S.askStage === s.id}">${esc(s.name)}</button>`).join('')}</div>
        <button class="btn btn-primary ml-auto">Search</button></div>
      </form>
      <div id="askResults"></div></div>`);
    $$('#askStage button').forEach((b) => b.onclick = () => { S.askStage = b.dataset.s; $$('#askStage button').forEach((x) => x.setAttribute('aria-pressed', x === b)); runAsk(); });
    $('#askForm').onsubmit = (e) => { e.preventDefault(); runAsk(); };
    if (S.askQ || S.askStage) runAsk();
    else $('#askResults').innerHTML = `<div class="text-center text-ink3"><p class="mb-3">Try a question like:</p><div class="flex flex-wrap justify-center gap-2">${['Which features did papers build for user behaviour?', 'How was SMOTE applied?', 'What results were reported?', 'learning rate'].map((q) => `<button class="chip hover:border-accent" data-q="${esc(q)}">${esc(q)}</button>`).join('')}</div></div>`;
    $$('[data-q]').forEach((b) => b.onclick = () => { $('#askQ').value = b.dataset.q; runAsk(); });
    $('#askQ').focus();
  }
  async function runAsk() {
    S.askQ = $('#askQ').value.trim();
    const box = $('#askResults');
    if (!S.askQ && !S.askStage) { box.innerHTML = ''; return; }
    const hits = await api(`/api/search?q=${encodeURIComponent(S.askQ)}&stage=${encodeURIComponent(S.askStage || '')}`);
    if (!hits.length) { box.innerHTML = empty('Nothing in your papers says that', 'Try other words, or add more papers. ResearchTrail only answers from passages it kept, so it never fills gaps with guesses.'); return; }
    const words = S.askQ.toLowerCase().split(/\W+/).filter((w) => w.length > 3);
    const mark = (t) => { let h = esc(t); for (const w of words) h = h.replace(new RegExp(`\\b(${w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\w*)`, 'gi'), '<mark class="rounded bg-accent/20 px-0.5 text-ink">$1</mark>'); return h; };
    const stageName = Object.fromEntries(S.state.stages.map((s) => [s.id, s.name]));
    box.innerHTML = `<p class="mb-3 text-[13px] text-ink3">${plural(hits.length, 'passage')} from ${plural(new Set(hits.map((h) => h.paper)).size, 'paper')}</p><ol class="space-y-3">${hits.map((h) => `
      <li class="panel-raised view-enter px-5 py-4"><p class="paper-text">${mark(h.text)}</p>
      <div class="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[12.5px] text-ink3"><a class="font-medium text-ink2 hover:underline" href="#/papers/${h.paper}">${esc(h.cite)}</a><span>${esc(h.section)}</span>${stageName[h.stage] ? `<span class="chip">${esc(stageName[h.stage])}</span>` : ''}${h.link ? `<a class="hover:underline" href="${esc(h.link)}" target="_blank" rel="noopener">original</a>` : ''}</div></li>`).join('')}</ol>`;
  }

  // ---------------- compare ----------------
  async function viewCompare() {
    const stage = S.arg || S.compareStage || 'features';
    S.compareStage = stage;
    const [t, cat] = await Promise.all([api(`/api/compare?stage=${stage}`), S.catalog ? Promise.resolve(S.catalog) : api('/api/catalog')]);
    S.catalog = cat;
    setTitle(`Compare: ${t.stage_name}`, 'What each paper did next to what your code does, matched through shared technique names.');
    const cov = Object.fromEntries(S.state.coverage.map((c) => [c.stage, c]));
    const tabs = `<div class="seg mb-5 flex-wrap" role="tablist">${S.state.stages.map((s) => `<a role="tab" href="#/compare/${s.id}" class="rounded-md px-3 py-1 text-[13px] ${s.id === stage ? 'bg-raised text-ink shadow-soft' : 'text-ink2 hover:text-ink'}" aria-selected="${s.id === stage}">${esc(s.name)} <span class="text-ink3">${cov[s.id] && (cov[s.id].paper_techniques || cov[s.id].mine) ? cov[s.id].paper_techniques + cov[s.id].mine - cov[s.id].shared : ''}</span></a>`).join('')}</div>`;
    if (!t.columns.length) {
      main(tabs + empty(`Nothing for ${t.stage_name} yet`, `No paper card or file in your code mentions a ${t.stage_name.toLowerCase()} technique ResearchTrail knows. Add papers, or name the unrecognised steps in your code below.`) + `<div class="mx-auto mt-6 grid max-w-[1100px] gap-4 lg:grid-cols-2">${unknownPanel(t, cat)}${addPanel(t, cat)}</div>`);
      wireCompare(t, cat); return;
    }
    const rows = [t.mine, ...t.rows];
    const head = t.columns.map((c) => `<th scope="col" class="min-w-[104px] max-w-[150px] px-2 pb-3 pt-1 text-center align-bottom text-[12.5px] font-medium leading-tight text-ink2"><span title="${c.papers} of ${t.rows.length} papers${c.mine ? ', and your code' : ''}">${esc(c.name)}</span></th>`).join('');
    const extraHead = t.stage === 'evaluation' ? 'Reported' : t.stage === 'training' ? 'Settings' : t.stage === 'data' ? 'Datasets' : '';
    const body = rows.map((r, ri) => {
      const mine = r.kind === 'mine';
      return `<tr class="${mine ? 'bg-mine/10' : 'border-t border-line'}">
        <th scope="row" class="sticky left-0 z-10 max-w-[240px] bg-raised px-3 py-2.5 text-left align-top font-normal" ${mine ? 'style="background: linear-gradient(rgb(var(--mine) / .1), rgb(var(--mine) / .1)), rgb(var(--raised))"' : ''}>
          ${mine ? `<div class="font-semibold text-mine">Mine</div><div class="text-[12px] text-ink3">${t.files_used.length ? esc(t.files_used.join(', ')) : 'no matching code yet'}</div>` : `<a href="#/papers/${r.id}" class="block font-medium hover:underline">${esc(r.label)}</a><div class="line-clamp-2 font-serif text-[12.5px] leading-snug text-ink3">${esc(r.title)}</div>`}
        </th>
        ${t.columns.map((c, ci) => { const ev = r.cells[c.id]; return `<td class="px-2 py-2.5 text-center align-middle">${ev ? `<button class="cell-dot rounded-full p-1.5 hover:bg-sunken" style="animation-delay:${reduced ? 0 : (ri * 40 + ci * 25)}ms" data-cell="${ri}|${esc(c.id)}" aria-label="${esc(r.label)}: ${esc(c.name)}"><span class="dot ${mine ? 'bg-mine' : ev.some((e) => e.source !== 'rules') ? 'bg-accent ring-2 ring-accent/30' : 'bg-accent'}"></span></button>` : '<span class="text-ink3/40">–</span>'}</td>`; }).join('')}
        ${extraHead ? `<td class="min-w-[160px] px-3 py-2.5 align-top text-[12.5px] text-ink2">${esc((r.facts || []).join(', '))}</td>` : ''}</tr>`;
    }).join('');
    const insight = (title, list, tone) => list.length ? `<div class="panel px-4 py-3"><div class="label mb-1.5">${title}</div><div class="flex flex-wrap gap-1.5">${list.map((x) => `<span class="chip ${tone}">${esc(x)}</span>`).join('')}</div></div>` : '';
    main(`${tabs}
      <div class="mb-4 grid gap-3 md:grid-cols-2">${insight('Only in your code', t.only_mine, 'chip-mine')}${insight('Most of your papers do this; your code doesn\'t', t.common_missing, 'chip-accent')}</div>
      <div class="panel-raised scroll-thin overflow-x-auto"><table class="w-full border-collapse text-[14px]"><thead><tr><th class="sticky left-0 z-10 bg-raised px-3 pb-3 text-left align-bottom text-[12.5px] font-medium text-ink3">${plural(t.rows.length, 'paper')} and your code</th>${head}${extraHead ? `<th class="px-3 pb-3 text-left align-bottom text-[12.5px] font-medium text-ink3">${extraHead}</th>` : ''}</tr></thead><tbody>${body}</tbody></table></div>
      <p class="mt-2 text-[12.5px] text-ink3">Select a dot to see the sentence or the line of code behind it. Ringed dots were added from a chat or model and checked against their passage.</p>
      <div class="mt-6 grid gap-4 lg:grid-cols-2">${unknownPanel(t, cat)}${addPanel(t, cat)}</div>`);
    wireCompare(t, cat, rows);
  }
  function unknownPanel(t, cat) {
    if (!t.unknown.length && !t.removed.length) return '';
    return `<section class="panel p-4">${t.unknown.length ? `<h3 class="text-[15px] font-semibold">Steps in your code ResearchTrail doesn't know</h3><p class="mb-3 mt-1 text-[13px] text-ink2">Name one once and it's recognised in every file, and in papers that use the same words, from then on.</p>
      <ul class="space-y-2">${t.unknown.slice(0, 8).map(([n, p, l]) => `<li class="flex flex-wrap items-center gap-2"><code class="rounded bg-sunken px-1.5 py-0.5">${esc(n)}</code><span class="text-[12.5px] text-ink3">${esc(p)}, line ${l}</span><button class="btn btn-quiet ml-auto !py-1" data-name="${esc(n)}">Name it</button></li>`).join('')}</ul>` : ''}
      ${t.removed.length ? `<h3 class="mt-4 text-[14px] font-semibold">Removed from your row</h3><ul class="mt-1 space-y-1 text-[13px]">${t.removed.map(([tech, p]) => `<li class="flex items-center gap-2">${esc(cat.techniques.find((x) => x.id === tech)?.name || tech)} <span class="text-ink3">in ${esc(p)}</span><button class="btn btn-quiet ml-auto !py-0.5" data-restore="${esc(tech)}|${esc(p)}">Put back</button></li>`).join('')}</ul>` : ''}</section>`;
  }
  function addPanel(t, cat) {
    const opts = cat.techniques.filter((x) => x.stage === t.stage).map((x) => `<option value="${esc(x.id)}">${esc(x.name)}</option>`).join('');
    return `<section class="panel p-4"><h3 class="text-[15px] font-semibold">Add to your row by hand</h3><p class="mb-3 mt-1 text-[13px] text-ink2">For something you did that isn't in code here, like a step done in a spreadsheet.</p>
      <form id="addMine" class="flex flex-wrap gap-2"><select id="mineTech" class="input !w-auto flex-1" aria-label="Technique">${opts}</select><input id="mineNote" class="input !w-auto flex-1" placeholder="Optional note" aria-label="Note"><button class="btn">Add</button></form></section>`;
  }
  function wireCompare(t, cat, rows = []) {
    $$('[data-cell]').forEach((b) => b.onclick = () => {
      const [ri, cid] = b.dataset.cell.split('|');
      const r = rows[+ri];
      const col = t.columns.find((c) => c.id === cid);
      const ev = r.cells[cid];
      const html = ev.map((e) => e.kind === 'code'
        ? `<figure class="mb-4"><figcaption class="mb-1 text-[12.5px] text-ink3">${esc(e.path)}${e.cell != null ? `, cell ${e.cell + 1}` : ''}, line ${e.line}${e.unit && !e.unit.startsWith('cell ') ? `, in ${esc(e.unit)}` : ''}</figcaption><pre class="scroll-thin overflow-x-auto rounded-lg bg-sunken px-3 py-2">${esc(e.text)}</pre><button class="btn btn-quiet btn-danger mt-2 !py-1" data-drop="${esc(cid)}|${esc(e.path)}">Not right: remove from my row</button></figure>`
        : e.kind === 'you' ? `<p class="mb-3">${esc(e.text)}</p>`
        : `<figure class="mb-4"><figcaption class="mb-1 text-[12px] text-ink3">${esc(e.passage)}. ${esc(SOURCE[e.source] || e.source)}</figcaption><blockquote class="paper-text border-l-2 border-accent pl-3">${esc(e.text)}</blockquote></figure>`).join('');
      drawer(`${r.kind === 'mine' ? 'Your code' : r.label}: ${col.name}`, html + (r.kind === 'paper' ? `<a class="btn" href="#/papers/${r.id}">Open the paper card</a>` : ''), (d) => {
        $$('[data-drop]', d).forEach((x) => x.onclick = async () => { const [technique, path] = x.dataset.drop.split('|'); await api('/api/mine/remove', { method: 'POST', body: { technique, path } }); closeDrawer(); toast('Removed from your row. You can put it back below the table.'); });
        $$('a', d).forEach((a) => a.addEventListener('click', closeDrawer));
      });
    });
    $$('[data-restore]').forEach((b) => b.onclick = async () => { const [technique, path] = b.dataset.restore.split('|'); await api('/api/mine/restore', { method: 'POST', body: { technique, path } }); });
    $$('[data-name]').forEach((b) => b.onclick = () => {
      const opts = S.state.stages.map((s) => `<option value="${s.id}" ${s.id === t.stage ? 'selected' : ''}>${esc(s.name)}</option>`).join('');
      drawer(`Name ${b.dataset.name}`, `<form id="nameForm" class="space-y-3"><p class="text-[14px] text-ink2">What does <code>${esc(b.dataset.name)}</code> do in your pipeline? Use the words a paper would use, so papers that mention it match too.</p>
        <label class="block"><span class="label">Name</span><input id="nmName" class="input mt-1" placeholder="Quantile transform" required></label>
        <label class="block"><span class="label">Stage</span><select id="nmStage" class="input mt-1">${opts}</select></label>
        <button class="btn btn-primary">Save to this project's catalog</button></form>`, (d) => {
        $('#nmName', d).focus();
        $('#nameForm', d).onsubmit = async (e) => { e.preventDefault(); try { await api('/api/catalog/name', { method: 'POST', body: { code: b.dataset.name, name: $('#nmName', d).value, stage: $('#nmStage', d).value } }); S.catalog = null; closeDrawer(); toast('Saved. It\'s recognised in your code and in papers from now on.', 'ok'); } catch (err) { toast(err.message, 'error'); } };
      });
    });
    const f = $('#addMine');
    if (f) f.onsubmit = async (e) => { e.preventDefault(); await api('/api/mine/add', { method: 'POST', body: { stage: t.stage, technique: $('#mineTech').value, note: $('#mineNote').value } }); toast('Added to your row.'); };
  }

  // ---------------- code history ----------------
  async function viewCode() {
    const code = await api('/api/code');
    const lineages = code.lineages.sort((a, b) => (b.last || 0) - (a.last || 0));
    const sel = lineages.some((l) => l.id === S.arg) ? S.arg : lineages.some((l) => l.id === S.codeLineage) ? S.codeLineage : lineages[0]?.id;
    S.codeLineage = sel;
    setTitle('Code history', `Every save in ${code.root} is kept. Your files are never moved, renamed or deleted.`);
    if (!lineages.length) { main(empty('No Python files or notebooks yet', 'Save a <code>.py</code> file or a notebook anywhere in this project folder. Each save becomes a version you can compare and restore. Nothing to install, no commands to learn.')); return; }
    const files = Object.fromEntries(code.files.map((f) => [f.path, f]));
    const snaps = await api(`/api/history?lineage=${encodeURIComponent(sel)}`);
    const cur = lineages.find((l) => l.id === sel);
    main(`<div class="grid gap-6 lg:grid-cols-[330px_1fr]">
      <ul class="scroll-thin flex max-h-[calc(100vh-170px)] flex-col gap-2 overflow-y-auto pr-1">${lineages.map((l) => `<li><a href="#/code/${encodeURIComponent(l.id)}" class="block rounded-card border px-4 py-3 transition ${l.id === sel ? 'border-mine/50 bg-raised shadow-soft' : 'border-line bg-surface hover:bg-raised'}">
        <div class="font-medium">${esc(l.name)}</div><div class="mt-0.5 text-[12.5px] text-ink3">${plural(l.versions, 'version')}${l.members.length > 1 ? `, ${l.members.length} file names` : ''}, ${ago(l.last)}</div></a></li>`).join('')}</ul>
      <section class="panel-raised p-5">
        <h2 class="text-[17px] font-semibold">${esc(cur.name)}</h2>
        <div class="mb-4 mt-2 flex flex-wrap gap-2">${cur.members.map((m) => { const f = files[m] || {}; return `<span class="chip ${f.current ? 'chip-mine' : ''}" title="${f.current ? 'The newest file: this one counts as yours' : f.superseded ? 'An older file name in this history' : 'Not in the folder any more; its history is kept'}">${esc(m)}${f.current ? ', current' : f.superseded ? ', older draft' : ', gone'}${cur.members.length > 1 ? ` <button class="ml-1 text-ink3 underline-offset-2 hover:text-ink hover:underline" data-split="${esc(m)}" title="This file isn't the same as the others: give it its own history">split off</button>` : ''}</span>`; }).join('')}</div>
        ${cur.members.map((m) => files[m]).filter((f) => f && (f.never_run || f.skipped_cells)).map((f) => `<p class="mb-3 rounded-lg bg-sunken px-3 py-2 text-[13px] text-ink2">${f.never_run ? `${esc(f.path)} has never been run, so all its cells are counted.` : `${plural(f.skipped_cells, 'cell')} in ${esc(f.path)} never ran and ${f.skipped_cells === 1 ? "isn't" : "aren't"} counted as yours.`}</p>`).join('')}
        <ol class="relative ml-2 border-l border-line pl-6">${snaps.map((s, i) => `<li class="mb-5"><span class="absolute -left-[6.5px] mt-1.5 h-3 w-3 rounded-full border-2 border-raised ${i === 0 ? 'bg-mine' : 'bg-line'}"></span>
          <div class="flex flex-wrap items-baseline gap-x-3"><span class="font-medium">${esc(s.path.split('/').pop())}</span><span class="text-[12.5px] text-ink3">${ago(s.time)}, <span class="text-good">+${s.added}</span> <span class="text-bad">−${s.removed}</span> lines</span></div>
          <ul class="mt-1 text-[14px] text-ink2">${s.summary.map((x) => `<li>${esc(x)}</li>`).join('')}</ul>
          <div class="mt-1.5 flex gap-2"><button class="btn btn-quiet !py-0.5 text-[13px]" data-diff="${s.id}">See changes</button>${i > 0 ? `<button class="btn btn-quiet !py-0.5 text-[13px]" data-restore-v="${s.id}" data-path="${esc(s.path)}">Restore this version</button>` : ''}</div></li>`).join('')}</ol>
        ${lineages.length > 1 ? `<form id="mergeForm" class="mt-2 flex flex-wrap items-center gap-2 border-t border-line pt-4 text-[13.5px]"><span class="text-ink2">Is another history really this same file?</span><select id="mergeFrom" class="input !w-auto" aria-label="History to join">${lineages.filter((l) => l.id !== cur.id).map((l) => `<option value="${esc(l.id)}">${esc(l.name)}</option>`).join('')}</select><button class="btn">Join into this one</button></form>` : ''}
      </section></div>`);
    $$('[data-diff]').forEach((b) => b.onclick = async () => {
      const d = await api(`/api/diff?to=${b.dataset.diff}`);
      const lines = d.map((x) => x.k === 'hunk' ? '<div class="my-2 border-t border-dashed border-line"></div>' : `<div class="${x.k === 'add' ? 'diff-add' : x.k === 'del' ? 'diff-del' : ''} whitespace-pre px-2"><span class="mr-2 select-none text-ink3">${x.k === 'add' ? '+' : x.k === 'del' ? '−' : ' '}</span>${esc(x.t)}</div>`).join('');
      drawer('Changes in this version', `<div class="scroll-thin overflow-x-auto rounded-lg bg-sunken py-2 font-mono text-[12.5px] leading-relaxed">${lines || '<p class="px-3 text-ink3">No line changes.</p>'}</div>`);
    });
    $$('[data-restore-v]').forEach((b) => b.onclick = async () => {
      if (!confirm(`Put this version back into ${b.dataset.path}? The current version is saved first, so you can undo this.`)) return;
      try { await api('/api/restore', { method: 'POST', body: { id: b.dataset.restoreV } }); toast(`Restored ${b.dataset.path}. Reopen it in your editor to see the change.`, 'ok'); } catch (e) { toast(e.message, 'error'); }
    });
    $$('[data-split]').forEach((b) => b.onclick = async (e) => { e.preventDefault(); try { await api('/api/lineage/split', { method: 'POST', body: { path: b.dataset.split } }); toast(`${b.dataset.split} now has its own history.`); } catch (err) { toast(err.message, 'error'); } });
    const mf = $('#mergeForm');
    if (mf) mf.onsubmit = async (e) => { e.preventDefault(); try { await api('/api/lineage/merge', { method: 'POST', body: { into: cur.id, from: $('#mergeFrom').value } }); toast('Joined into one history.'); } catch (err) { toast(err.message, 'error'); } };
  }

  // ---------------- prompt packs ----------------
  async function viewPacks() {
    setTitle('Prompt packs', 'One block of text with the right paper passages and your code, sized for a free AI chat.');
    const stages = S.state.stages;
    const budget = S.packBudget || 8000;
    main(`<div class="grid gap-6 lg:grid-cols-[380px_1fr]">
      <form id="packForm" class="panel-raised h-fit space-y-4 p-5">
        <label class="block"><span class="label">What do you want help with?</span><textarea id="packTask" rows="5" class="input mt-1" placeholder="My feature engineering lags behind the papers. Suggest features I could add and show the code.">${esc(S.packTask || '')}</textarea></label>
        <label class="block"><span class="label">Focus on</span><select id="packStage" class="input mt-1"><option value="">Every stage</option>${stages.map((s) => `<option value="${s.id}" ${S.packStage === s.id ? 'selected' : ''}>${esc(s.name)}</option>`).join('')}</select></label>
        <div><span class="label">Size</span><div class="seg mt-1 flex-wrap" id="packBudget">${[[4000, 'Small'], [8000, 'Medium'], [16000, 'Large'], [32000, 'Very large']].map(([v, n]) => `<button type="button" data-b="${v}" aria-pressed="${v === budget}" title="About ${v.toLocaleString()} tokens">${n}</button>`).join('')}</div><p class="mt-1 text-[12px] text-ink3">Small fits most free chats. Larger packs include more passages and code.</p></div>
        <button class="btn btn-primary w-full">Build the pack</button>
      </form>
      <section id="packOut">${empty('Your pack appears here', 'Describe the task, pick a stage, and build. You get one block to paste into ChatGPT, Claude, Gemini or any other chat.')}</section></div>`);
    $$('#packBudget button').forEach((b) => b.onclick = () => { S.packBudget = +b.dataset.b; $$('#packBudget button').forEach((x) => x.setAttribute('aria-pressed', x === b)); });
    $('#packForm').onsubmit = async (e) => {
      e.preventDefault();
      S.packTask = $('#packTask').value; S.packStage = $('#packStage').value;
      try {
        const p = await api('/api/pack', { method: 'POST', body: { task: S.packTask, stage: S.packStage, budget: S.packBudget || 8000 } });
        const pct = Math.min(100, Math.round((p.tokens / p.budget) * 100));
        $('#packOut').innerHTML = `<div class="panel-raised view-enter p-5">
          <div class="mb-3 flex flex-wrap items-center gap-3"><div class="min-w-[200px] flex-1"><div class="mb-1 text-[13px] text-ink2">About ${p.tokens.toLocaleString()} of ${p.budget.toLocaleString()} tokens</div><div class="h-2 overflow-hidden rounded-full bg-sunken"><div class="h-full rounded-full bg-accent transition-all duration-700" style="width:0" data-w="${pct}%"></div></div></div><button class="btn btn-primary" id="copyPack">Copy the pack</button></div>
          <div class="mb-3 flex flex-wrap gap-1.5">${p.included.map((x) => `<span class="chip chip-accent">${esc(x)}</span>`).join('')}${p.left_out.map((x) => `<span class="chip" title="Didn't fit the size you chose">left out: ${esc(x)}</span>`).join('')}</div>
          <pre class="scroll-thin max-h-[60vh] overflow-auto whitespace-pre-wrap rounded-lg bg-sunken p-4 text-[12.5px] leading-relaxed">${esc(p.text)}</pre></div>`;
        requestAnimationFrame(() => setTimeout(() => { const b = $('[data-w]'); if (b) b.style.width = b.dataset.w; }, 40));
        $('#copyPack').onclick = async () => { await copy(p.text); toast('Copied. Paste it into your chat.', 'ok'); };
      } catch (err) { toast(err.message, 'error'); }
    };
  }

  // ---------------- settings ----------------
  async function viewSettings() {
    const c = await api('/api/settings');
    setTitle('Settings', S.state.project.root);
    main(`<form id="setForm" class="panel-raised max-w-[680px] space-y-5 p-6">
      <label class="block"><span class="font-medium">Code folder</span><span class="mt-0.5 block text-[13px] text-ink3">A folder inside this project. A single dot means the whole project folder.</span><input id="sCode" class="input mt-2" value="${esc(c.code_dir)}"></label>
      <label class="block"><span class="font-medium">Email for Unpaywall</span><span class="mt-0.5 block text-[13px] text-ink3">Unpaywall finds legal free copies of DOI papers and asks for an email in case of heavy use. It isn't sent anywhere else.</span><input id="sEmail" type="email" class="input mt-2" value="${esc(c.unpaywall_email)}" placeholder="you@university.edu"></label>
      <label class="block"><span class="font-medium">Local model (optional)</span><span class="mt-0.5 block text-[13px] text-ink3">An Ollama model name such as <code>qwen2.5:3b</code>, used to improve paper cards on this computer. Leave it empty to skip.</span><input id="sModel" class="input mt-2" value="${esc(c.ollama_model)}" placeholder="qwen2.5:3b"></label>
      <label class="flex items-start gap-3"><input id="sArchive" type="checkbox" class="mt-1 h-4 w-4" style="accent-color: rgb(var(--accent))" ${c.archive_links ? 'checked' : ''}><span><span class="font-medium">Save an archived copy of blog and web links</span><span class="mt-0.5 block text-[13px] text-ink3">Asks the Wayback Machine to keep the page, so the link still works if the site disappears. arXiv and DOI links are permanent already.</span></span></label>
      <div class="flex flex-wrap gap-2 border-t border-line pt-5"><button class="btn btn-primary">Save settings</button><button type="button" class="btn btn-quiet" id="closeProject">Switch project</button></div>
    </form>`);
    $('#setForm').onsubmit = async (e) => {
      e.preventDefault();
      try { await api('/api/settings', { method: 'POST', body: { code_dir: $('#sCode').value, unpaywall_email: $('#sEmail').value, ollama_model: $('#sModel').value, archive_links: $('#sArchive').checked } }); toast('Settings saved.', 'ok'); } catch (err) { toast(err.message, 'error'); }
    };
    $('#closeProject').onclick = closeProject;
  }

  // ---------------- welcome ----------------
  async function renderWelcome() {
    const p = await api('/api/projects').catch(() => ({ recent: [] }));
    $('#welcome').innerHTML = `<div class="view-enter">
      <svg viewBox="0 0 320 80" class="mb-6 h-16 w-auto" aria-hidden="true"><path d="M10 60 C 60 60, 70 20, 120 20 S 190 60, 240 60 S 290 30, 310 30" fill="none" stroke="rgb(var(--accent) / .45)" stroke-width="3" stroke-linecap="round" class="trail-path"/><circle cx="10" cy="60" r="7" fill="rgb(var(--accent))" class="station" style="animation-delay:.3s"/><circle cx="120" cy="20" r="7" fill="rgb(var(--accent))" class="station" style="animation-delay:.6s"/><circle cx="240" cy="60" r="7" fill="rgb(var(--accent))" class="station" style="animation-delay:.9s"/><circle cx="310" cy="30" r="9" fill="rgb(var(--mine))" class="station" style="animation-delay:1.2s"/></svg>
      <h1 class="mb-3 max-w-[18ch] font-serif text-[40px] leading-[1.1]">Keep what you read next to what you build.</h1>
      <p class="mb-8 max-w-[56ch] text-[16px] text-ink2">ResearchTrail turns the papers you read into cards, keeps every version of your code without git, and shows what each paper did next to what your code does. Everything stays in your project folder.</p>
      <div class="panel-raised p-5">
        <label for="openPath" class="mb-2 block font-medium">Open your research project folder</label>
        <form id="openForm" class="flex flex-wrap gap-2"><input id="openPath" class="input min-w-[240px] flex-1" placeholder="C:\\Users\\you\\thesis  or  /home/you/thesis"><button type="button" class="btn" id="pickBtn">Browse…</button><button class="btn btn-primary">Open</button></form>
        ${p.recent?.length ? `<div class="mt-4"><div class="label mb-1.5">Recent</div><ul class="space-y-1">${p.recent.map((r) => `<li><button class="text-left text-[14px] text-ink2 hover:text-ink hover:underline" data-recent="${esc(r)}">${esc(r)}</button></li>`).join('')}</ul></div>` : ''}
      </div>
      <p class="mt-5 text-[14px] text-ink2">New here? <button id="demoBtn" class="font-medium text-accent hover:underline">Open the demo project</button>. It has three invented example papers and some messy drafts to explore.</p></div>`;
    const open = async (path) => { try { await api('/api/project/open', { method: 'POST', body: { path } }); S.open = null; } catch (e) { toast(e.message, 'error'); } };
    $('#openForm').onsubmit = (e) => { e.preventDefault(); open($('#openPath').value); };
    $('#pickBtn').onclick = async () => { try { const r = await api('/api/project/pick', { method: 'POST' }); $('#openPath').value = r.path; open(r.path); } catch (e) { toast(e.message, 'error'); } };
    $$('[data-recent]').forEach((b) => b.onclick = () => open(b.dataset.recent));
    $('#demoBtn').onclick = openDemo;
  }
  async function openDemo() {
    try { toast('Preparing the demo project…'); await api('/api/project/demo', { method: 'POST' }); S.open = null; location.hash = '#/overview'; } catch (e) { toast(e.message, 'error'); }
  }
  async function closeProject() { await api('/api/project/close', { method: 'POST' }); S.open = null; S.lastCoverage = {}; }

  // ---------------- global controls ----------------
  async function addLink(link) {
    try { await api('/api/papers', { method: 'POST', body: { link } }); $('#addLink').value = ''; } catch (e) { toast(e.message, 'error'); }
  }
  $('#addForm').onsubmit = (e) => { e.preventDefault(); const v = $('#addLink').value.trim(); if (v) addLink(v); };
  $('#projectBtn').onclick = closeProject;
  const themes = ['system', 'light', 'dark'];
  function applyTheme(t) { if (t === 'system') { delete document.documentElement.dataset.theme; localStorage.removeItem('rt-theme'); } else { document.documentElement.dataset.theme = t; localStorage.setItem('rt-theme', t); } $('#themeBtn').textContent = `Theme: ${t}`; }
  $('#themeBtn').onclick = () => { const cur = localStorage.getItem('rt-theme') || 'system'; applyTheme(themes[(themes.indexOf(cur) + 1) % 3]); };
  applyTheme(localStorage.getItem('rt-theme') || 'system');
  addEventListener('keydown', (e) => { if (e.key === '/' && !/INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName)) { e.preventDefault(); $('#addLink')?.focus(); } });
  pulse();
})();
