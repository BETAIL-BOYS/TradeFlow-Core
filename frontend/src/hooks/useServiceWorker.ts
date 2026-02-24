'use client';

import { useEffect, useState, useCallback } from 'react';

interface ServiceWorkerRegistration {
  installing: ServiceWorker | null;
  waiting: ServiceWorker | null;
  active: ServiceWorker | null;
}

export function useServiceWorker() {
  const [registration, setRegistration] = useState<ServiceWorkerRegistration | null>(null);
  const [isSupported, setIsSupported] = useState(false);
  const [isReady, setIsReady] = useState(false);
  const [updateAvailable, setUpdateAvailable] = useState(false);

  useEffect(() => {
    if (typeof window !== 'undefined' && 'serviceWorker' in navigator) {
      setIsSupported(true);
      
      // Register service worker
      registerServiceWorker();
    }
  }, []);

  const registerServiceWorker = useCallback(async () => {
    try {
      const reg = await navigator.serviceWorker.register('/sw.js', {
        scope: '/'
      });

      setRegistration(reg);
      setIsReady(true);

      console.log('Service Worker registered with scope:', reg.scope);

      // Check for updates
      reg.addEventListener('updatefound', () => {
        const newWorker = reg.installing;
        if (newWorker) {
          newWorker.addEventListener('statechange', () => {
            if (newWorker.state === 'installed' && navigator.serviceWorker.controller) {
              setUpdateAvailable(true);
            }
          });
        }
      });

      // Handle controller change (new service worker activated)
      navigator.serviceWorker.addEventListener('controllerchange', () => {
        console.log('Controller changed - reloading page');
        window.location.reload();
      });

      // Listen for messages from service worker
      navigator.serviceWorker.addEventListener('message', (event) => {
        console.log('Message from service worker:', event.data);
        
        // Handle custom events
        if (event.data.type === 'SYNC_STATUS') {
          window.dispatchEvent(new CustomEvent('sync-status', {
            detail: event.data.payload
          }));
        }
      });

    } catch (error) {
      console.error('Service Worker registration failed:', error);
    }
  }, []);

  const applyUpdate = useCallback(() => {
    if (registration && registration.waiting) {
      registration.waiting.postMessage({ type: 'SKIP_WAITING' });
    }
  }, [registration]);

  const unregisterServiceWorker = useCallback(async () => {
    if (registration) {
      await registration.unregister();
      setRegistration(null);
      setIsReady(false);
      console.log('Service Worker unregistered');
    }
  }, [registration]);

  const sendMessageToSW = useCallback(async (message: any) => {
    if (registration && registration.active) {
      registration.active.postMessage(message);
    }
  }, [registration]);

  const triggerBackgroundSync = useCallback(async (tag: string) => {
    if (registration && 'sync' in registration) {
      try {
        await registration.sync.register(tag);
        console.log(`Background sync registered for tag: ${tag}`);
      } catch (error) {
        console.error('Background sync registration failed:', error);
      }
    }
  }, [registration]);

  const getNotificationsPermission = useCallback(async () => {
    if ('Notification' in window) {
      return Notification.permission;
    }
    return 'denied';
  }, []);

  const requestNotificationsPermission = useCallback(async () => {
    if ('Notification' in window) {
      const permission = await Notification.requestPermission();
      return permission;
    }
    return 'denied';
  }, []);

  const showNotification = useCallback(async (title: string, options?: NotificationOptions) => {
    if (registration && 'showNotification' in registration) {
      try {
        await registration.showNotification(title, {
          icon: '/icons/icon-192x192.png',
          badge: '/icons/icon-96x96.png',
          ...options
        });
      } catch (error) {
        console.error('Failed to show notification:', error);
      }
    }
  }, [registration]);

  return {
    isSupported,
    isReady,
    registration,
    updateAvailable,
    applyUpdate,
    unregisterServiceWorker,
    sendMessageToSW,
    triggerBackgroundSync,
    getNotificationsPermission,
    requestNotificationsPermission,
    showNotification,
  };
}
