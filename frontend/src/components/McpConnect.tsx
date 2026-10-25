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
      <p className="font-medium">Data v Claude a ChatGPT</p>
      <p className="mt-1 text-ink-2">Přes MCP server se můžeš na svoje data ptát v chatu. Přístup je jen ke čtení a jen k tvým datům.</p>
      <div className="mt-3 flex items-center gap-1 rounded-lg border bg-surface-2 py-1 pl-2.5 pr-1">
        <code className="min-w-0 flex-1 truncate text-[13px]">{url}</code>
        <Button variant="ghost" size="icon-sm" onClick={copy} aria-label="Zkopírovat URL" title="Zkopírovat URL">
          {copied ? <Check className="text-good" /> : <Copy />}
        </Button>
      </div>
      <dl className="mt-3 space-y-2 text-ink-2">
        <div>
          <dt className="font-medium text-ink">Claude</dt>
          <dd>Settings → Connectors → Add custom connector, vlož URL.</dd>
        </div>
        <div>
          <dt className="font-medium text-ink">ChatGPT</dt>
          <dd>Settings → Apps & Connectors → Advanced → Developer mode, pak Create, vlož URL a zvol OAuth.</dd>
        </div>
      </dl>
      <p className="mt-3 text-muted">Pak se přihlásíš stejným Google účtem jako sem.</p>
    </div>
  );
}
