const lightbox = document.querySelector('.lightbox');
const lightboxImage = lightbox?.querySelector('img');
const closeButton = lightbox?.querySelector('.lightbox-close');

document.querySelectorAll('[data-lightbox]').forEach((button) => {
  button.addEventListener('click', () => {
    if (!lightbox || !lightboxImage) return;
    lightboxImage.src = button.dataset.lightbox;
    lightboxImage.alt = button.querySelector('img')?.alt || 'BoogieBox screenshot';
    lightbox.showModal();
  });
});

closeButton?.addEventListener('click', () => lightbox?.close());

lightbox?.addEventListener('click', (event) => {
  if (event.target === lightbox) lightbox.close();
});
