const grid = document.getElementById('agent-grid');
for (let i = 1; i <= 24; i++) {
  const tile = document.createElement('button');
  tile.className = 'mw-agent-tile';
  tile.textContent = String(i).padStart(2, '0');
  grid.appendChild(tile);
}

document.querySelectorAll('.mw-nav-item').forEach((item) => {
  item.addEventListener('click', () => {
    document.querySelectorAll('.mw-nav-item').forEach((n) => n.classList.remove('active'));
    item.classList.add('active');

    const target = item.dataset.screen;
    document.querySelectorAll('.screen').forEach((s) => s.classList.remove('active'));
    document.getElementById(target)?.classList.add('active');
    document.getElementById('screen-label').textContent = target[0].toUpperCase() + target.slice(1);
  });
});
