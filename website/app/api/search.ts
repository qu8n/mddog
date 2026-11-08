import { createFromSource } from 'fumadocs-core/search/server';
import { source } from '@/lib/source';

const server = createFromSource(source);

// The site has no server, so the build saves the whole index and the browser searches it.
export async function loader() {
  return server.staticGET();
}
