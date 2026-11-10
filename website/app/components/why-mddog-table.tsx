import { Table } from '@cloudflare/kumo/components/table';
import { CheckIcon, XIcon } from '@phosphor-icons/react';
import { whyColumns as columns, whyNote, whyRows as rows } from '@/components/why-mddog-data';
import { cn } from '@/lib/cn';

// The mddog column is the one place in the table with emphasis: a surface step and full-strength checks.
const mddogColumn = 'bg-fd-accent/40';

function Mark({ value, strong }: { value: boolean | string; strong: boolean }) {
  if (typeof value === 'string') return <span className="text-fd-muted-foreground">{value}</span>;
  const Icon = value ? CheckIcon : XIcon;
  return (
    <Icon
      size={16}
      weight={value && strong ? 'bold' : 'regular'}
      aria-label={value ? 'Yes' : 'No'}
      className={cn(
        'inline-block align-middle',
        value && strong ? 'text-fd-primary' : 'text-fd-muted-foreground',
        !value && 'opacity-60',
      )}
    />
  );
}

/** A check mark, as the comparison table draws a yes. For MDX tables such as Compatibility's. */
export function Yes() {
  return <Mark value strong />;
}

/** An X, as the comparison table draws a no. */
export function No() {
  return <Mark value={false} strong={false} />;
}

/** The "Why mddog?" comparison table and the note on what it was compared to. Used on the home page and in Quick start. */
export function WhyMddogTable({ className }: { className?: string }) {
  return (
    <div className={cn('not-prose text-sm', className)}>
      <div className="overflow-x-auto rounded-xl border">
        <Table className="min-w-xl [&_td]:text-sm [&_th]:text-sm">
          <Table.Header>
            <Table.Row>
              <Table.Head />
              {columns.map((c, i) => (
                <Table.Head key={c} className={cn('w-1/5 text-center', i === 0 && cn(mddogColumn, 'text-fd-foreground'))}>
                  {c}
                </Table.Head>
              ))}
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {rows.map((row) => (
              <Table.Row key={row.label}>
                <Table.Cell>{row.label}</Table.Cell>
                {row.cells.map((cell, i) => (
                  <Table.Cell key={columns[i]} className={cn('text-center', i === 0 && mddogColumn)}>
                    <Mark value={cell} strong={i === 0} />
                  </Table.Cell>
                ))}
              </Table.Row>
            ))}
          </Table.Body>
        </Table>
      </div>
      <p className="mt-4 italic text-fd-muted-foreground">
        {whyNote}
      </p>
    </div>
  );
}
