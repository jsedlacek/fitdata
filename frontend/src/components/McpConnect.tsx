import { useState } from 'react';
import { ArrowUpRight, Check, Copy } from 'lucide-react';
import { DEMO } from '@/lib/demo';
import { Button } from './ui/button';

/** How to read the data from Claude or ChatGPT through the MCP server at /mcp; in the demo, what that looks like. */
export function McpConnect() {
  if (DEMO) return <DemoChat />;
  const url = `${location.origin}/mcp`;
  const [copied, setCopied] = useState(false);
  const copy = () =>
    void navigator.clipboard.writeText(url).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });

  return (
    <div className="text-sm">
      <p className="font-medium">Your data in Claude and ChatGPT</p>
      <p className="mt-1 text-ink-2">Through the MCP server you can ask about your data in a chat. Access is read-only and limited to your data.</p>
      <div className="mt-3 flex items-center gap-1 rounded-lg border bg-surface-2 py-1 pl-2.5 pr-1">
        <code className="min-w-0 flex-1 truncate text-[13px]">{url}</code>
        <Button variant="ghost" size="icon-sm" onClick={copy} aria-label="Copy URL" title="Copy URL">
          {copied ? <Check className="text-good" /> : <Copy />}
        </Button>
      </div>
      <dl className="mt-3 space-y-2 text-ink-2">
        <div>
          <dt className="font-medium text-ink">Claude</dt>
          <dd>Settings → Connectors → Add custom connector, paste the URL.</dd>
        </div>
        <div>
          <dt className="font-medium text-ink">ChatGPT</dt>
          <dd>Settings → Apps & Connectors → Advanced → Developer mode, then Create, paste the URL and choose OAuth.</dd>
        </div>
      </dl>
      <p className="mt-3 text-muted">Then sign in with the same Google account as here.</p>
    </div>
  );
}

/** The demo can't connect an assistant: what asking looks like with your own data, and how to set it up. */
function DemoChat() {
  return (
    <div className="text-sm">
      <p className="font-medium">Ask Claude or ChatGPT about your data</p>
      <p className="mt-1 text-ink-2">
        With your own data, Fitdata is a connector for Claude, ChatGPT and other assistants that speak MCP. Add it once in the assistant&apos;s settings,
        then ask in plain words, like &ldquo;Why am I so tired today?&rdquo; or &ldquo;Do I sleep better after evening runs?&rdquo;
      </p>
      <p className="mt-2 text-ink-2">The assistant can only read your data, nobody else&apos;s, and can&apos;t change anything.</p>
      <a href="/docs/ask-ai" target="_blank" rel="noopener" className="mt-3 inline-flex items-center gap-0.5 font-medium text-s1 hover:underline">
        How to connect Claude or ChatGPT <ArrowUpRight className="size-3.5" />
      </a>
      <a href="/auth/login" className="mt-4 inline-flex h-9 w-full items-center justify-center rounded-lg bg-ink text-[13px] font-medium text-page hover:opacity-90">
        Connect your Fitbit
      </a>
    </div>
  );
}
