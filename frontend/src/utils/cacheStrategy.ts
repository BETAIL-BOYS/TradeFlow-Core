// Cache strategy utilities for PWA implementation

export interface CacheOptions {
  cacheName: string;
  maxAge?: number; // in milliseconds
  maxEntries?: number;
  networkTimeout?: number; // in milliseconds
}

export interface CacheEntry {
  response: Response;
  timestamp: number;
  url: string;
}

export class CacheStrategy {
  private cacheName: string;
  private maxAge: number;
  private maxEntries: number;
  private networkTimeout: number;

  constructor(options: CacheOptions) {
    this.cacheName = options.cacheName;
    this.maxAge = options.maxAge || 24 * 60 * 60 * 1000; // 24 hours default
    this.maxEntries = options.maxEntries || 100;
    this.networkTimeout = options.networkTimeout || 3000; // 3 seconds default
  }

  // Cache First Strategy - tries cache first, then network
  async cacheFirst(request: Request): Promise<Response> {
    const cache = await caches.open(this.cacheName);
    const cachedResponse = await cache.match(request);

    if (cachedResponse && !this.isExpired(cachedResponse)) {
      return cachedResponse;
    }

    try {
      const networkResponse = await this.fetchWithTimeout(request);
      if (networkResponse.ok) {
        await this.putInCache(cache, request, networkResponse);
      }
      return networkResponse;
    } catch (error) {
      if (cachedResponse) {
        return cachedResponse;
      }
      throw error;
    }
  }

  // Network First Strategy - tries network first, then cache
  async networkFirst(request: Request): Promise<Response> {
    const cache = await caches.open(this.cacheName);

    try {
      const networkResponse = await this.fetchWithTimeout(request);
      if (networkResponse.ok) {
        await this.putInCache(cache, request, networkResponse);
      }
      return networkResponse;
    } catch (error) {
      const cachedResponse = await cache.match(request);
      if (cachedResponse) {
        return cachedResponse;
      }
      throw error;
    }
  }

  // Stale While Revalidate Strategy - serves from cache, updates in background
  async staleWhileRevalidate(request: Request): Promise<Response> {
    const cache = await caches.open(this.cacheName);
    const cachedResponse = await cache.match(request);

    const networkPromise = this.fetchWithTimeout(request)
      .then(async (networkResponse) => {
        if (networkResponse.ok) {
          await this.putInCache(cache, request, networkResponse);
        }
        return networkResponse;
      })
      .catch(() => null);

    if (cachedResponse) {
      // Trigger network update in background
      networkPromise.catch(() => {}); // Ignore errors for background update
      return cachedResponse;
    }

    // If no cache, wait for network
    const networkResponse = await networkPromise;
    if (networkResponse) {
      return networkResponse;
    }

    throw new Error('No cache available and network request failed');
  }

  // Network Only Strategy - always tries network, never cache
  async networkOnly(request: Request): Promise<Response> {
    return this.fetchWithTimeout(request);
  }

  // Cache Only Strategy - always serves from cache, never network
  async cacheOnly(request: Request): Promise<Response> {
    const cache = await caches.open(this.cacheName);
    const cachedResponse = await cache.match(request);

    if (cachedResponse) {
      return cachedResponse;
    }

    throw new Error('No cache entry found');
  }

  private async fetchWithTimeout(request: Request): Promise<Response> {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.networkTimeout);

    try {
      const response = await fetch(request, {
        signal: controller.signal,
      });
      clearTimeout(timeoutId);
      return response;
    } catch (error) {
      clearTimeout(timeoutId);
      throw error;
    }
  }

  private async putInCache(cache: Cache, request: Request, response: Response): Promise<void> {
    // Clone the response since it can only be consumed once
    const responseClone = response.clone();
    
    // Add timestamp to response headers for expiration checking
    const headers = new Headers(responseClone.headers);
    headers.set('sw-cached-at', Date.now().toString());
    
    const modifiedResponse = new Response(responseClone.body, {
      status: responseClone.status,
      statusText: responseClone.statusText,
      headers,
    });

    await cache.put(request, modifiedResponse);
    
    // Clean up old entries if maxEntries is exceeded
    await this.cleanupCache(cache);
  }

  private isExpired(response: Response): boolean {
    const cachedAt = response.headers.get('sw-cached-at');
    if (!cachedAt) return true;

    const cacheAge = Date.now() - parseInt(cachedAt);
    return cacheAge > this.maxAge;
  }

  private async cleanupCache(cache: Cache): Promise<void> {
    const requests = await cache.keys();
    
    if (requests.length <= this.maxEntries) return;

    // Get all entries with their timestamps
    const entries: CacheEntry[] = [];
    
    for (const request of requests) {
      const response = await cache.match(request);
      if (response) {
        const cachedAt = response.headers.get('sw-cached-at');
        entries.push({
          response,
          timestamp: cachedAt ? parseInt(cachedAt) : 0,
          url: request.url,
        });
      }
    }

    // Sort by timestamp (oldest first)
    entries.sort((a, b) => a.timestamp - b.timestamp);

    // Remove oldest entries to maintain maxEntries
    const entriesToRemove = entries.slice(0, entries.length - this.maxEntries);
    
    for (const entry of entriesToRemove) {
      await cache.delete(entry.url);
    }
  }

  // Clear all cache entries
  async clearCache(): Promise<void> {
    const cache = await caches.open(this.cacheName);
    const requests = await cache.keys();
    
    for (const request of requests) {
      await cache.delete(request);
    }
  }

  // Get cache size information
  async getCacheInfo(): Promise<{
    entries: number;
    size: number;
    oldestEntry: number | null;
    newestEntry: number | null;
  }> {
    const cache = await caches.open(this.cacheName);
    const requests = await cache.keys();
    
    let totalSize = 0;
    let oldestTimestamp = Date.now();
    let newestTimestamp = 0;

    for (const request of requests) {
      const response = await cache.match(request);
      if (response) {
        const cachedAt = response.headers.get('sw-cached-at');
        const timestamp = cachedAt ? parseInt(cachedAt) : 0;
        
        oldestTimestamp = Math.min(oldestTimestamp, timestamp);
        newestTimestamp = Math.max(newestTimestamp, timestamp);
        
        // Estimate size (this is approximate)
        const responseClone = response.clone();
        const text = await responseClone.text();
        totalSize += text.length;
      }
    }

    return {
      entries: requests.length,
      size: totalSize,
      oldestEntry: oldestTimestamp === Date.now() ? null : oldestTimestamp,
      newestEntry: newestTimestamp === 0 ? null : newestTimestamp,
    };
  }
}

// Predefined cache strategies for different types of content
export const cacheStrategies = {
  // For static assets that rarely change
  staticAssets: new CacheStrategy({
    cacheName: 'static-assets-v1',
    maxAge: 7 * 24 * 60 * 60 * 1000, // 7 days
    maxEntries: 200,
  }),

  // For API responses that change occasionally
  apiResponses: new CacheStrategy({
    cacheName: 'api-responses-v1',
    maxAge: 5 * 60 * 1000, // 5 minutes
    maxEntries: 50,
  }),

  // For user data that should be fresh
  userData: new CacheStrategy({
    cacheName: 'user-data-v1',
    maxAge: 60 * 1000, // 1 minute
    maxEntries: 20,
  }),

  // For proofs and verification data
  proofsData: new CacheStrategy({
    cacheName: 'proofs-data-v1',
    maxAge: 30 * 60 * 1000, // 30 minutes
    maxEntries: 100,
  }),

  // For images and media
  media: new CacheStrategy({
    cacheName: 'media-v1',
    maxAge: 30 * 24 * 60 * 60 * 1000, // 30 days
    maxEntries: 50,
  }),
};

// Helper function to determine which strategy to use based on URL
export function getCacheStrategyForUrl(url: string): CacheStrategy {
  if (url.includes('/_next/static/') || url.includes('/icons/') || url.endsWith('.css') || url.endsWith('.js')) {
    return cacheStrategies.staticAssets;
  }
  
  if (url.includes('/api/')) {
    if (url.includes('/proofs') || url.includes('/verification')) {
      return cacheStrategies.proofsData;
    }
    if (url.includes('/user') || url.includes('/profile')) {
      return cacheStrategies.userData;
    }
    return cacheStrategies.apiResponses;
  }
  
  if (url.match(/\.(jpg|jpeg|png|gif|webp|svg)$/i)) {
    return cacheStrategies.media;
  }
  
  // Default to stale-while-revalidate for HTML pages
  return new CacheStrategy({
    cacheName: 'pages-v1',
    maxAge: 10 * 60 * 1000, // 10 minutes
    maxEntries: 20,
  });
}

// Utility function to warm up cache with critical resources
export async function warmupCache(urls: string[]): Promise<void> {
  const cachePromises = urls.map(async (url) => {
    try {
      const strategy = getCacheStrategyForUrl(url);
      const request = new Request(url);
      await strategy.staleWhileRevalidate(request);
    } catch (error) {
      console.warn(`Failed to warm up cache for ${url}:`, error);
    }
  });

  await Promise.allSettled(cachePromises);
}
