// Content-free push: the text is fixed here; the push carries no payload.
self.addEventListener('push', (e) => {
  e.waitUntil(self.registration.showNotification('Bastion', { body: '1 request waiting', tag: 'bastion-waiting' }));
});
self.addEventListener('notificationclick', (e) => {
  e.notification.close();
  e.waitUntil(clients.matchAll({ type: 'window' }).then((l) => (l.length ? l[0].focus() : clients.openWindow('/'))));
});
