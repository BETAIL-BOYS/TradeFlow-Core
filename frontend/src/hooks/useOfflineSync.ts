'use client';

import { useState, useEffect, useCallback } from 'react';
import { useServiceWorker } from './useServiceWorker';

interface OfflineData {
  proofs: any[];
  verifications: any[];
}

interface SyncQueueItem {
  id: string;
  type: 'proof' | 'verification';
  data: any;
  timestamp: number;
  retryCount: number;
}

export function useOfflineSync() {
  const [isOnline, setIsOnline] = useState(true);
  const [pendingItems, setPendingItems] = useState<SyncQueueItem[]>([]);
  const [isSyncing, setIsSyncing] = useState(false);
  const [lastSyncTime, setLastSyncTime] = useState<Date | null>(null);
  const [syncError, setSyncError] = useState<string | null>(null);
  
  const { triggerBackgroundSync, isReady: swReady } = useServiceWorker();

  useEffect(() => {
    const updateOnlineStatus = () => {
      const online = navigator.onLine;
      setIsOnline(online);
      
      if (online && pendingItems.length > 0) {
        // Trigger sync when coming back online
        syncPendingItems();
      }
    };

    // Initialize online status
    updateOnlineStatus();

    // Listen for online/offline events
    window.addEventListener('online', updateOnlineStatus);
    window.addEventListener('offline', updateOnlineStatus);

    // Load pending items from IndexedDB on mount
    loadPendingItems();

    return () => {
      window.removeEventListener('online', updateOnlineStatus);
      window.removeEventListener('offline', updateOnlineStatus);
    };
  }, [pendingItems.length]);

  const openDB = useCallback((): Promise<IDBDatabase> => {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open('verinode-offline', 1);
      
      request.onerror = () => reject(request.error);
      request.onsuccess = () => resolve(request.result);
      
      request.onupgradeneeded = () => {
        const db = request.result;
        if (!db.objectStoreNames.contains('proofs')) {
          db.createObjectStore('proofs', { keyPath: 'id' });
        }
        if (!db.objectStoreNames.contains('verifications')) {
          db.createObjectStore('verifications', { keyPath: 'id' });
        }
        if (!db.objectStoreNames.contains('syncQueue')) {
          const syncQueueStore = db.createObjectStore('syncQueue', { keyPath: 'id' });
          syncQueueStore.createIndex('type', 'type', { unique: false });
          syncQueueStore.createIndex('timestamp', 'timestamp', { unique: false });
        }
      };
    });
  }, []);

  const loadPendingItems = useCallback(async () => {
    try {
      const db = await openDB();
      const transaction = db.transaction(['syncQueue'], 'readonly');
      const store = transaction.objectStore('syncQueue');
      const request = store.getAll();
      
      request.onsuccess = () => {
        setPendingItems(request.result || []);
      };
      
      db.close();
    } catch (error) {
      console.error('Error loading pending items:', error);
    }
  }, [openDB]);

  const addToSyncQueue = useCallback(async (type: 'proof' | 'verification', data: any) => {
    const item: SyncQueueItem = {
      id: `${type}_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
      type,
      data,
      timestamp: Date.now(),
      retryCount: 0,
    };

    try {
      const db = await openDB();
      const transaction = db.transaction(['syncQueue'], 'readwrite');
      const store = transaction.objectStore('syncQueue');
      await store.add(item);
      
      setPendingItems(prev => [...prev, item]);
      
      // Also store in the appropriate store for offline access
      const dataStore = db.transaction([type + 's'], 'readwrite').objectStore(type + 's');
      await dataStore.add({ ...data, id: item.id });
      
      db.close();
      
      // Try to sync immediately if online
      if (isOnline && swReady) {
        triggerBackgroundSync(`background-sync-${type}s`);
      }
      
    } catch (error) {
      console.error('Error adding to sync queue:', error);
      throw error;
    }
  }, [openDB, isOnline, swReady, triggerBackgroundSync]);

  const saveProofOffline = useCallback(async (proof: any) => {
    return addToSyncQueue('proof', proof);
  }, [addToSyncQueue]);

  const saveVerificationOffline = useCallback(async (verification: any) => {
    return addToSyncQueue('verification', verification);
  }, [addToSyncQueue]);

  const syncPendingItems = useCallback(async () => {
    if (!isOnline || isSyncing || pendingItems.length === 0) return;

    setIsSyncing(true);
    setSyncError(null);

    try {
      const db = await openDB();
      const transaction = db.transaction(['syncQueue'], 'readwrite');
      const store = transaction.objectStore('syncQueue');
      
      const itemsToSync = [...pendingItems];
      const successfulSyncs: string[] = [];
      const failedSyncs: SyncQueueItem[] = [];

      for (const item of itemsToSync) {
        try {
          const endpoint = item.type === 'proof' ? '/api/proofs' : '/api/verification';
          const response = await fetch(endpoint, {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
            },
            body: JSON.stringify(item.data),
          });

          if (response.ok) {
            successfulSyncs.push(item.id);
            
            // Remove from the specific data store
            const dataStore = db.transaction([item.type + 's'], 'readwrite').objectStore(item.type + 's');
            await dataStore.delete(item.id);
          } else {
            // Increment retry count and keep in queue if under max retries
            const updatedItem = { ...item, retryCount: item.retryCount + 1 };
            if (updatedItem.retryCount < 3) {
              await store.put(updatedItem);
              failedSyncs.push(updatedItem);
            } else {
              // Remove from queue after max retries
              await store.delete(item.id);
              console.error(`Max retries exceeded for item ${item.id}`);
            }
          }
        } catch (error) {
          console.error(`Failed to sync item ${item.id}:`, error);
          
          // Increment retry count
          const updatedItem = { ...item, retryCount: item.retryCount + 1 };
          if (updatedItem.retryCount < 3) {
            await store.put(updatedItem);
            failedSyncs.push(updatedItem);
          } else {
            await store.delete(item.id);
          }
        }
      }

      setPendingItems(failedSyncs);
      setLastSyncTime(new Date());
      
      if (failedSyncs.length > 0) {
        setSyncError(`${failedSyncs.length} items failed to sync`);
      }
      
      db.close();
    } catch (error) {
      console.error('Error during sync:', error);
      setSyncError('Sync failed. Please try again.');
    } finally {
      setIsSyncing(false);
    }
  }, [isOnline, isSyncing, pendingItems, openDB]);

  const getOfflineData = useCallback(async (): Promise<OfflineData> => {
    try {
      const db = await openDB();
      const proofsTransaction = db.transaction(['proofs'], 'readonly');
      const verificationsTransaction = db.transaction(['verifications'], 'readonly');
      
      const proofs = await new Promise<any[]>((resolve) => {
        const request = proofsTransaction.objectStore('proofs').getAll();
        request.onsuccess = () => resolve(request.result || []);
      });
      
      const verifications = await new Promise<any[]>((resolve) => {
        const request = verificationsTransaction.objectStore('verifications').getAll();
        request.onsuccess = () => resolve(request.result || []);
      });
      
      db.close();
      
      return { proofs, verifications };
    } catch (error) {
      console.error('Error getting offline data:', error);
      return { proofs: [], verifications: [] };
    }
  }, [openDB]);

  const clearOfflineData = useCallback(async () => {
    try {
      const db = await openDB();
      const stores = ['proofs', 'verifications', 'syncQueue'];
      
      for (const storeName of stores) {
        const transaction = db.transaction([storeName], 'readwrite');
        const store = transaction.objectStore(storeName);
        await store.clear();
      }
      
      setPendingItems([]);
      setLastSyncTime(new Date());
      
      db.close();
    } catch (error) {
      console.error('Error clearing offline data:', error);
      throw error;
    }
  }, [openDB]);

  const getOfflineProofs = useCallback(async () => {
    const data = await getOfflineData();
    return data.proofs;
  }, [getOfflineData]);

  const getOfflineVerifications = useCallback(async () => {
    const data = await getOfflineData();
    return data.verifications;
  }, [getOfflineData]);

  return {
    isOnline,
    pendingItems,
    isSyncing,
    lastSyncTime,
    syncError,
    saveProofOffline,
    saveVerificationOffline,
    syncPendingItems,
    getOfflineData,
    getOfflineProofs,
    getOfflineVerifications,
    clearOfflineData,
  };
}
