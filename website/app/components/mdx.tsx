import { Banner } from '@cloudflare/kumo/components/banner';
import { CheckCircleIcon, InfoIcon, LightbulbIcon, WarningCircleIcon, WarningIcon } from '@phosphor-icons/react';
import defaultMdxComponents from 'fumadocs-ui/mdx';
import * as previews from '@/components/mddog-previews';
import * as mousePreviews from '@/components/mouse-previews';
import { ReloadPreview } from '@/components/reload-preview';
import { ThemesPreview } from '@/components/theme-previews';
import { No, WhyMddogTable, Yes } from '@/components/why-mddog-table';
import type { MDXComponents } from 'mdx/types';
import type { ComponentProps, ReactNode } from 'react';

type CalloutType = 'info' | 'warn' | 'warning' | 'error' | 'success' | 'idea';

const callouts = {
  info: { variant: 'default', icon: <InfoIcon weight="fill" /> },
  warn: { variant: 'alert', icon: <WarningIcon weight="fill" /> },
  warning: { variant: 'alert', icon: <WarningIcon weight="fill" /> },
  error: { variant: 'error', icon: <WarningCircleIcon weight="fill" /> },
  success: { variant: 'secondary', icon: <CheckCircleIcon weight="fill" /> },
  idea: { variant: 'secondary', icon: <LightbulbIcon weight="fill" /> },
} as const;

/** Fumadocs' `<Callout>` in MDX, drawn as a Kumo Banner. */
function Callout({
  type = 'info',
  title,
  children,
  className,
}: { type?: CalloutType; title?: ReactNode; children?: ReactNode } & Pick<ComponentProps<'div'>, 'className'>) {
  const { variant, icon } = callouts[type] ?? callouts.info;
  return (
    <Banner
      variant={variant}
      icon={icon}
      title={typeof title === 'string' ? title : undefined}
      // An element is used as is; anything else gets wrapped in a <p>, which MDX paragraphs can't sit in.
      description={<div>{children}</div>}
      className={`not-prose my-6 [&_p]:my-0 ${className ?? ''}`}
    />
  );
}

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    Callout,
    WhyMddogTable,
    Yes,
    No,
    ThemesPreview,
    ReloadPreview,
    ...previews,
    ...mousePreviews,
    ...components,
  } satisfies MDXComponents;
}

export const useMDXComponents = getMDXComponents;

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>;
}
