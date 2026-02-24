'use client';

import { useState, useEffect } from 'react';

interface SyncStatus {
  isOnline: boolean;
  lastSyncTime: Date | null;
  pendingSyncs: number;
  isSyncing: boolean;
  syncError: string | null;
}

export default function SyncStatus() {
  const [syncStatus, setSyncStatus] = useState<SyncStatus>({
    isOnline: navigator.onLine,
    lastSyncTime: null,
    pendingSyncs: 0,
    isSyncing: false,
    syncError: null,
  });

  const [showDetails, setShowDetails] = useState(false);

  useEffect(() => {
    const updateOnlineStatus = () => {
      setSyncStatus(prev => ({
        ...prev,
        isOnline: navigator.onLine,
      }));
    };

    const handleSyncEvent = (event: CustomEvent) => {
      setSyncStatus(prev => ({
        ...prev,
        ...event.detail,
      }));
    };

    // Listen for online/offline events
    window.addEventListener('online', updateOnlineStatus);
    window.addEventListener('offline', updateOnlineStatus);
    
    // Listen for custom sync events from service worker
    window.addEventListener('sync-status', handleSyncEvent as EventListener);

    // Check for pending syncs on load
    checkPendingSyncs();

    return () => {
      window.removeEventListener('online', updateOnlineStatus);
      window.removeEventListener('offline', updateOnlineStatus);
      window.removeEventListener('sync-status', handleSyncEvent as EventListener);
    };
  }, []);

  const checkPendingSyncs = async () => {
    try {
      const db = await openDB();
      const proofsStore = db.transaction('proofs', 'readonly').objectStore('proofs');
      const verificationsStore = db.transaction('verifications', 'readonly').objectStore('verifications');
      
      const pendingProofs = await proofsStore.count();
      const pendingVerifications = await verificationsStore.count();
      
      setSyncStatus(prev => ({
        ...prev,
        pendingSyncs: pendingProofs + pendingVerifications,
      }));
      
      db.close();
    } catch (error) {
      console.error('Error checking pending syncs:', error);
    }
  };

  const openDB = (): Promise<IDBDatabase> => {
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
      };
    });
  };

  const triggerManualSync = async () => {
    if (!syncStatus.isOnline || syncStatus.isSyncing) return;

    setSyncStatus(prev => ({ ...prev, isSyncing: true, syncError: null }));

    try {
      // Register background sync if supported
      if ('serviceWorker' in navigator && 'SyncManager' in window) {
        const registration = await navigator.serviceWorker.ready;
        await registration.sync.register('background-sync-proofs');
        await registration.sync.register('background-sync-verification');
      } else {
        // Fallback: manual sync
        await performManualSync();
      }
    } catch (error) {
      console.error('Manual sync failed:', error);
      setSyncStatus(prev => ({
        ...prev,
        syncError: 'Sync failed. Please try again.',
        isSyncing: false,
      }));
    }
  };

  const performManualSync = async () => {
    // This would implement the same logic as the service worker sync
    // but executed in the main thread for browsers without background sync
    try {
      const db = await openDB();
      
      // Sync proofs
      const proofsStore = db.transaction('proofs', 'readwrite').objectStore('proofs');
      const allProofs = await proofsStore.getAll();
      
      for (const proof of allProofs) {
        try {
          const response = await fetch('/api/proofs', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(proof),
          });
          
          if (response.ok) {
            await proofsStore.delete(proof.id);
          }
        } catch (error) {
          console.error('Failed to sync proof:', proof.id, error);
        }
      }
      
      // Sync verifications
      const verificationsStore = db.transaction('verifications', 'readwrite').objectStore('verifications');
      const allVerifications = await verificationsStore.getAll();
      
      for (const verification of allVerifications) {
        try {
          const response = await fetch('/api/verification', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(verification),
          });
          
          if (response.ok) {
            await verificationsStore.delete(verification.id);
          }
        } catch (error) {
          console.error('Failed to sync verification:', verification.id, error);
        }
      }
      
      db.close();
      
      setSyncStatus(prev => ({
        ...prev,
        isSyncing: false,
        lastSyncTime: new Date(),
        pendingSyncs: 0,
      }));
    } catch (error) {
      setSyncStatus(prev => ({
        ...prev,
        isSyncing: false,
        syncError: 'Manual sync failed',
      }));
    }
  };

  const getStatusColor = () => {
    if (syncStatus.syncError) return 'text-red-600';
    if (syncStatus.isSyncing) return 'text-yellow-600';
    if (!syncStatus.isOnline) return 'text-gray-500';
    if (syncStatus.pendingSyncs > 0) return 'text-orange-600';
    return 'text-green-600';
  };

  const getStatusIcon = () => {
    if (syncStatus.syncError) {
      return (
        <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
      );
    }
    if (syncStatus.isSyncing) {
      return (
        <svg className="w-4 h-4 animate-spin" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
        </svg>
      );
    }
    if (!syncStatus.isOnline) {
      return (
        <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M18.364 5.636l-3.536 3.536m0 5.656l3.536 3.536M9.172 9.172L5.636 5.636m3.536 9.192L5.636 18.364M12 2.25a9.75 9.75 0 109.75 9.75A9.75 9.75 0 0012 2.25z" />
        </svg>
      );
    }
    if (syncStatus.pendingSyncs > 0) {
      return (
        <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
      );
    }
    return (
      <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
      </svg>
    );
  };

  const getStatusText = () => {
    if (syncStatus.syncError) return 'Sync Error';
    if (syncStatus.isSyncing) return 'Syncing...';
    if (!syncStatus.isOnline) return 'Offline';
    if (syncStatus.pendingSyncs > 0) return `${syncStatus.pendingSyncs} Pending`;
    return 'Synced';
  };

  return (
    <div className="fixed bottom-4 right-4 z-40">
      <div className="bg-white rounded-lg shadow-lg border border-gray-200 p-3 min-w-48">
        <button
          onClick={() => setShowDetails(!showDetails)}
          className="flex items-center justify-between w-full text-left"
        >
          <div className="flex items-center space-x-2">
            <div className={getStatusColor()}>
              {getStatusIcon()}
            </div>
            <span className={`text-sm font-medium ${getStatusColor()}`}>
              {getStatusText()}
            </span>
          </div>
          
          <svg
            className={`w-4 h-4 text-gray-400 transition-transform ${
              showDetails ? 'rotate-180' : ''
            }`}
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
          </svg>
        </button>

        {showDetails && (
          <div className="mt-3 space-y-2 text-xs text-gray-600">
            {syncStatus.lastSyncTime && (
              <div>
                Last sync: {syncStatus.lastSyncTime.toLocaleTimeString()}
              </div>
            )}
            
            {syncStatus.pendingSyncs > 0 && (
              <div className="text-orange-600">
                {syncStatus.pendingSyncs} items waiting to sync
              </div>
            )}
            
            {syncStatus.syncError && (
              <div className="text-red-600">
                Error: {syncStatus.syncError}
              </div>
            )}
            
            {syncStatus.isOnline && syncStatus.pendingSyncs > 0 && !syncStatus.isSyncing && (
              <button
                onClick={triggerManualSync}
                className="w-full px-2 py-1 bg-primary-500 text-white rounded hover:bg-primary-600 transition-colors"
              >
                Sync Now
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
