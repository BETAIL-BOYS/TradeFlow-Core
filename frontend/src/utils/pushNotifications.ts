// Push notification utilities for PWA

export interface NotificationPayload {
  title: string;
  body: string;
  icon?: string;
  badge?: string;
  tag?: string;
  data?: any;
  actions?: NotificationAction[];
  requireInteraction?: boolean;
  silent?: boolean;
  vibrate?: number[];
}

export interface NotificationAction {
  action: string;
  title: string;
  icon?: string;
}

export interface PushSubscription {
  endpoint: string;
  keys: {
    p256dh: string;
    auth: string;
  };
}

export class PushNotificationManager {
  private registration: ServiceWorkerRegistration | null = null;
  private subscription: PushSubscription | null = null;

  constructor() {
    this.initialize();
  }

  private async initialize() {
    if ('serviceWorker' in navigator && 'PushManager' in window) {
      try {
        this.registration = await navigator.serviceWorker.ready;
        await this.loadSubscription();
      } catch (error) {
        console.error('Failed to initialize push notifications:', error);
      }
    }
  }

  // Request permission for notifications
  async requestPermission(): Promise<NotificationPermission> {
    if (!('Notification' in window)) {
      throw new Error('This browser does not support notifications');
    }

    let permission = Notification.permission;

    if (permission === 'default') {
      permission = await Notification.requestPermission();
    }

    return permission;
  }

  // Check if notifications are supported and permitted
  async isSupported(): Promise<boolean> {
    return 'serviceWorker' in navigator && 
           'PushManager' in window && 
           'Notification' in window &&
           Notification.permission === 'granted';
  }

  // Subscribe to push notifications
  async subscribe(): Promise<PushSubscription | null> {
    if (!this.registration) {
      throw new Error('Service worker not registered');
    }

    try {
      const permission = await this.requestPermission();
      if (permission !== 'granted') {
        throw new Error('Notification permission not granted');
      }

      // In production, you would use your actual VAPID public key
      const applicationServerKey = this.urlBase64ToUint8Array(
        process.env.NEXT_PUBLIC_VAPID_PUBLIC_KEY || 
        'BMxzFTmF3i9j9A5DhxGJ5g0Q7Sz8Z9fX7g8J7v8k9l0m1n2o3p4q5r6s7t8u9v0w'
      );

      const subscription = await this.registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey,
      });

      this.subscription = subscription;
      await this.saveSubscription(subscription);
      
      console.log('Push subscription successful:', subscription);
      return subscription;
    } catch (error) {
      console.error('Failed to subscribe to push notifications:', error);
      throw error;
    }
  }

  // Unsubscribe from push notifications
  async unsubscribe(): Promise<boolean> {
    if (!this.subscription) {
      return true;
    }

    try {
      const success = await this.subscription.unsubscribe();
      if (success) {
        this.subscription = null;
        await this.removeSubscription();
        console.log('Successfully unsubscribed from push notifications');
      }
      return success;
    } catch (error) {
      console.error('Failed to unsubscribe from push notifications:', error);
      return false;
    }
  }

  // Get current subscription
  getSubscription(): PushSubscription | null {
    return this.subscription;
  }

  // Show a local notification
  async showLocalNotification(payload: NotificationPayload): Promise<void> {
    const permission = await this.requestPermission();
    if (permission !== 'granted') {
      throw new Error('Notification permission not granted');
    }

    const options: NotificationOptions = {
      body: payload.body,
      icon: payload.icon || '/icons/icon-192x192.png',
      badge: payload.badge || '/icons/icon-96x96.png',
      tag: payload.tag,
      data: payload.data,
      requireInteraction: payload.requireInteraction || false,
      silent: payload.silent || false,
    };

    if (payload.actions) {
      options.actions = payload.actions;
    }

    if (payload.vibrate) {
      options.vibrate = payload.vibrate;
    }

    if (this.registration) {
      await this.registration.showNotification(payload.title, options);
    } else {
      // Fallback to browser notification
      new Notification(payload.title, options);
    }
  }

  // Send a push notification (server-side simulation)
  async sendPushNotification(payload: NotificationPayload): Promise<void> {
    if (!this.subscription) {
      throw new Error('No active subscription');
    }

    // In a real implementation, this would send the payload to your server
    // which would then send the push notification via web push protocol
    console.log('Sending push notification:', payload);
    
    // For demo purposes, show a local notification
    await this.showLocalNotification(payload);
  }

  // Schedule a notification (using setTimeout for demo)
  scheduleNotification(
    payload: NotificationPayload, 
    delayMs: number
  ): { id: number; cancel: () => void } {
    const id = setTimeout(async () => {
      try {
        await this.showLocalNotification(payload);
      } catch (error) {
        console.error('Failed to show scheduled notification:', error);
      }
    }, delayMs);

    return {
      id,
      cancel: () => clearTimeout(id),
    };
  }

  // Handle notification click
  static handleNotificationClick(event: NotificationEvent): void {
    const notification = event.notification;
    const action = event.action;

    console.log('Notification clicked:', notification, 'Action:', action);

    if (action === 'explore') {
      // Open specific page
      clients.openWindow('/proofs');
    } else if (action === 'dismiss') {
      // Just close the notification
      notification.close();
    } else {
      // Default action - open the app
      clients.openWindow('/');
    }

    notification.close();
  }

  // Handle notification close
  static handleNotificationClose(event: NotificationEvent): void {
    console.log('Notification closed:', event.notification);
  }

  // Convert URL base64 to Uint8Array (for VAPID key)
  private urlBase64ToUint8Array(base64String: string): Uint8Array {
    const padding = '='.repeat((4 - base64String.length % 4) % 4);
    const base64 = (base64String + padding)
      .replace(/-/g, '+')
      .replace(/_/g, '/');

    const rawData = window.atob(base64);
    const outputArray = new Uint8Array(rawData.length);

    for (let i = 0; i < rawData.length; ++i) {
      outputArray[i] = rawData.charCodeAt(i);
    }

    return outputArray;
  }

  // Save subscription to IndexedDB
  private async saveSubscription(subscription: PushSubscription): Promise<void> {
    try {
      const db = await this.openDB();
      const transaction = db.transaction(['pushSubscription'], 'readwrite');
      const store = transaction.objectStore('pushSubscription');
      await store.put({
        id: 'current',
        subscription: JSON.parse(JSON.stringify(subscription)),
        timestamp: Date.now(),
      });
      db.close();
    } catch (error) {
      console.error('Failed to save subscription:', error);
    }
  }

  // Load subscription from IndexedDB
  private async loadSubscription(): Promise<void> {
    try {
      const db = await this.openDB();
      const transaction = db.transaction(['pushSubscription'], 'readonly');
      const store = transaction.objectStore('pushSubscription');
      const result = await store.get('current');
      
      if (result) {
        this.subscription = result.subscription;
      }
      
      db.close();
    } catch (error) {
      console.error('Failed to load subscription:', error);
    }
  }

  // Remove subscription from IndexedDB
  private async removeSubscription(): Promise<void> {
    try {
      const db = await this.openDB();
      const transaction = db.transaction(['pushSubscription'], 'readwrite');
      const store = transaction.objectStore('pushSubscription');
      await store.delete('current');
      db.close();
    } catch (error) {
      console.error('Failed to remove subscription:', error);
    }
  }

  // Open IndexedDB for subscription storage
  private openDB(): Promise<IDBDatabase> {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open('verinode-push', 1);
      
      request.onerror = () => reject(request.error);
      request.onsuccess = () => resolve(request.result);
      
      request.onupgradeneeded = () => {
        const db = request.result;
        if (!db.objectStoreNames.contains('pushSubscription')) {
          db.createObjectStore('pushSubscription', { keyPath: 'id' });
        }
      };
    });
  }
}

// Predefined notification templates
export const notificationTemplates = {
  verificationComplete: (proofId: string): NotificationPayload => ({
    title: 'Verification Complete',
    body: `Your proof ${proofId} has been successfully verified.`,
    icon: '/icons/icon-192x192.png',
    tag: `verification-${proofId}`,
    data: { proofId, type: 'verification' },
    actions: [
      { action: 'view', title: 'View Proof' },
      { action: 'dismiss', title: 'Dismiss' },
    ],
    requireInteraction: true,
    vibrate: [200, 100, 200],
  }),

  verificationFailed: (proofId: string, reason: string): NotificationPayload => ({
    title: 'Verification Failed',
    body: `Your proof ${proofId} failed verification: ${reason}`,
    icon: '/icons/icon-192x192.png',
    tag: `verification-failed-${proofId}`,
    data: { proofId, type: 'verification-failed', reason },
    actions: [
      { action: 'retry', title: 'Retry' },
      { action: 'dismiss', title: 'Dismiss' },
    ],
    requireInteraction: true,
    vibrate: [200, 100, 200, 100, 200],
  }),

  newProofAvailable: (proofId: string): NotificationPayload => ({
    title: 'New Proof Available',
    body: `A new proof ${proofId} is available for review.`,
    icon: '/icons/icon-192x192.png',
    tag: `new-proof-${proofId}`,
    data: { proofId, type: 'new-proof' },
    actions: [
      { action: 'view', title: 'View Proof' },
      { action: 'dismiss', title: 'Dismiss' },
    ],
    vibrate: [100, 50, 100],
  }),

  syncComplete: (count: number): NotificationPayload => ({
    title: 'Sync Complete',
    body: `${count} items have been successfully synced.`,
    icon: '/icons/icon-192x192.png',
    tag: 'sync-complete',
    data: { type: 'sync', count },
    silent: true,
  }),

  offlineMode: (): NotificationPayload => ({
    title: 'Offline Mode',
    body: 'You are now offline. Changes will be synced when you reconnect.',
    icon: '/icons/icon-192x192.png',
    tag: 'offline-mode',
    data: { type: 'offline' },
    vibrate: [50],
  }),

  onlineMode: (): NotificationPayload => ({
    title: 'Back Online',
    body: 'You are back online. Syncing your changes...',
    icon: '/icons/icon-192x192.png',
    tag: 'online-mode',
    data: { type: 'online' },
    vibrate: [100],
  }),
};

// Singleton instance
export const pushNotificationManager = new PushNotificationManager();

// Utility functions for common notification scenarios
export const notifyVerificationComplete = async (proofId: string) => {
  try {
    await pushNotificationManager.showLocalNotification(
      notificationTemplates.verificationComplete(proofId)
    );
  } catch (error) {
    console.error('Failed to show verification complete notification:', error);
  }
};

export const notifyVerificationFailed = async (proofId: string, reason: string) => {
  try {
    await pushNotificationManager.showLocalNotification(
      notificationTemplates.verificationFailed(proofId, reason)
    );
  } catch (error) {
    console.error('Failed to show verification failed notification:', error);
  }
};

export const notifyNewProofAvailable = async (proofId: string) => {
  try {
    await pushNotificationManager.showLocalNotification(
      notificationTemplates.newProofAvailable(proofId)
    );
  } catch (error) {
    console.error('Failed to show new proof notification:', error);
  }
};

export const notifySyncComplete = async (count: number) => {
  try {
    await pushNotificationManager.showLocalNotification(
      notificationTemplates.syncComplete(count)
    );
  } catch (error) {
    console.error('Failed to show sync complete notification:', error);
  }
};

export const notifyOfflineMode = async () => {
  try {
    await pushNotificationManager.showLocalNotification(
      notificationTemplates.offlineMode()
    );
  } catch (error) {
    console.error('Failed to show offline mode notification:', error);
  }
};

export const notifyOnlineMode = async () => {
  try {
    await pushNotificationManager.showLocalNotification(
      notificationTemplates.onlineMode()
    );
  } catch (error) {
    console.error('Failed to show online mode notification:', error);
  }
};
