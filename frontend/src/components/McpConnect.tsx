import { useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { Button } from './ui/button';

/** How to read the data from Claude or ChatGPT through the MCP server at /mcp. */
export function McpConnect() {
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
