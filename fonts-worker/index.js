// Serves the licensed Berkeley Mono files to the mddog site only.
// Browsers always send Origin when CSS loads a font from another origin,
// so a missing or foreign Origin means another site or a direct download.
const ALLOWED = /^(https:\/\/([a-z0-9-]+\.)?mddog\.pages\.dev|http:\/\/localhost:517[3-5])$/;

export default {
  async fetch(request, env) {
    const origin = request.headers.get('Origin') ?? '';
    if (!ALLOWED.test(origin)) return new Response('Forbidden', { status: 403 });

    const asset = await env.ASSETS.fetch(request);
    const response = new Response(asset.body, asset);
    response.headers.set('Access-Control-Allow-Origin', origin);
    response.headers.set('Vary', 'Origin');
    // Never pin a 404 from a bad deploy in browser caches.
    response.headers.set('Cache-Control', asset.ok ? 'public, max-age=31536000, immutable' : 'no-store');
    return response;
  },
};
