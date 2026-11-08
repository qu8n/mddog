import type { LinkComponentProps } from '@cloudflare/kumo';
import { forwardRef } from 'react';
import { Link } from 'react-router';

/** Lets Kumo's Link and LinkButton navigate with React Router. External URLs stay plain links. */
export const RouterLink = forwardRef<HTMLAnchorElement, LinkComponentProps>(function RouterLink(
  { href = '', ...props },
  ref,
) {
  if (/^[a-z]+:|^\/\//i.test(href)) return <a ref={ref} href={href} {...props} />;
  return <Link ref={ref} to={href} {...props} />;
});
