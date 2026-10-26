// Full-text search over the docs. The index is built in the browser on first use, by
// rendering every page into a detached element and splitting its text at the headings.
import { createElement } from 'react';
import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';
import { PAGES, type Page } from './pages';

export type Entry = {
  page: Page;
  /** Section id; '' for the page itself. */
  id: string;
  title: string;
  text: string;
  /** Normalized title and text, for matching; the page's keywords count as its title. */
  nTitle: string;
  nText: string;
};

/** Lower case without accents, so "sp02" or "Rémy" match as expected. */
export const normalize = (s: string) =>
  s
    .normalize('NFD')
    .replace(/\p{M}/gu, '')
    .replace(/₂/g, '2')
    .toLowerCase();

let index: Entry[] | null = null;

/** The index if it is built already. */
export const builtIndex = () => index;

/** Builds the index; outside of React rendering and effects, where `flushSync` can't render. */
export function buildIndex(): Entry[] {
  if (index) return index;
  const out: Entry[] = [];
  for (const page of PAGES) {
    const el = document.createElement('div');
    const root = createRoot(el);
    flushSync(() => root.render(createElement(page.Body)));
    let cur = { id: '', title: page.title, text: `${page.summary} ` };
    const sections = [cur];
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      if (n instanceof HTMLHeadingElement && (n.tagName === 'H2' || n.tagName === 'H3')) {
        // The heading's own text without its "#" anchor link.
        const title = [...n.childNodes].filter((c) => !(c instanceof HTMLAnchorElement)).map((c) => c.textContent).join('').trim();
        cur = { id: n.id, title, text: '' };
        sections.push(cur);
      } else if (n.nodeType === Node.TEXT_NODE && !n.parentElement?.closest('h2, h3')) {
        cur.text += n.textContent;
      } else if (n instanceof HTMLElement && (/^(P|LI|TD|TH|DT|DD|DIV|FIGCAPTION)$/.test(n.tagName) || n.classList.contains('block'))) {
        // Blocks end words; inline elements don't.
        cur.text += ' ';
      }
    }
    root.unmount();
    for (const s of sections) {
      const text = s.text.replace(/\s+/g, ' ').trim();
      const keys = s.id ? '' : ` ${page.keywords ?? ''}`;
      out.push({ page, id: s.id, title: s.title, text, nTitle: normalize(s.title + keys), nText: normalize(text) });
    }
  }
  return (index = out);
}

export type Hit = { entry: Entry; score: number; snippet: { before: string; match: string; after: string } | null };

/** Entries matching every word of the query, best first. */
export function search(entries: Entry[], query: string, limit = 12): Hit[] {
  const words = normalize(query).split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  if (!words.length) return [];
  const phrase = words.join(' ');
  const hits: Hit[] = [];
  for (const e of entries) {
    const pageTitle = normalize(e.page.title);
    let score = 0;
    let ok = true;
    for (const w of words) {
      const inTitle = e.nTitle.indexOf(w);
      const inPage = pageTitle.indexOf(w);
      const inText = e.nText.indexOf(w);
      if (inTitle < 0 && inPage < 0 && inText < 0) {
        ok = false;
        break;
      }
      // Word starts count more than matches inside a word.
      const atStart = (s: string, i: number) => i === 0 || /[^\p{L}\p{N}]/u.test(s[i - 1]);
      if (inTitle >= 0) score += atStart(e.nTitle, inTitle) ? 12 : 6;
      if (inPage >= 0) score += atStart(pageTitle, inPage) ? 4 : 2;
      if (inText >= 0) score += Math.min(3, e.nText.split(w).length - 1) * (atStart(e.nText, inText) ? 1 : 0.5);
    }
    if (!ok) continue;
    // The words together, as typed, beat the same words apart.
    if (words.length > 1 && e.nTitle.includes(phrase)) score += 10;
    if (words.length > 1 && e.nText.includes(phrase)) score += 5;
    if (e.id === '') score += 1;
    hits.push({ entry: e, score, snippet: snippet(e, words.length > 1 && e.nText.includes(phrase) ? [phrase] : words) });
  }
  return hits.sort((a, b) => b.score - a.score).slice(0, limit);
}

/** About 90 characters around the first word found in the text. Normalizing keeps lengths, so indexes carry over. */
function snippet(e: Entry, words: string[]): Hit['snippet'] {
  let at = -1;
  let len = 0;
  for (const w of words) {
    const i = e.nText.indexOf(w);
    if (i >= 0 && (at < 0 || i < at)) [at, len] = [i, w.length];
  }
  if (at < 0) return e.text ? { before: '', match: '', after: e.text.slice(0, 110) + (e.text.length > 110 ? '…' : '') } : null;
  const from = Math.max(0, e.text.lastIndexOf(' ', Math.max(0, at - 40)) + 1);
  const to = Math.min(e.text.length, at + len + 70);
  return {
    before: (from > 0 ? '…' : '') + e.text.slice(from, at),
    match: e.text.slice(at, at + len),
    after: e.text.slice(at + len, to) + (to < e.text.length ? '…' : ''),
  };
}
