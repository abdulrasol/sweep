// Sweep Interactive Mockup and UI Logic

document.addEventListener('DOMContentLoaded', () => {
  // Mobile Navigation Toggle
  const menuToggle = document.querySelector('.menu-toggle');
  const navLinks = document.querySelector('.nav-links');
  
  if (menuToggle && navLinks) {
    menuToggle.addEventListener('click', () => {
      navLinks.classList.toggle('open');
      // Simple burger to X animation
      const spans = menuToggle.querySelectorAll('span');
      if (navLinks.classList.contains('open')) {
        spans[0].style.transform = 'rotate(45deg) translate(6px, 6px)';
        spans[1].style.opacity = '0';
        spans[2].style.transform = 'rotate(-45deg) translate(6px, -6px)';
      } else {
        spans[0].style.transform = 'none';
        spans[1].style.opacity = '1';
        spans[2].style.transform = 'none';
      }
    });
  }

  // Header Scroll Effect
  const header = document.querySelector('header');
  window.addEventListener('scroll', () => {
    if (window.scrollY > 50) {
      header.classList.add('scrolled');
    } else {
      header.classList.remove('scrolled');
    }
  });

  // FAQ Accordion
  const faqItems = document.querySelectorAll('.faq-item');
  faqItems.forEach(item => {
    const question = item.querySelector('.faq-question');
    question.addEventListener('click', () => {
      const isActive = item.classList.contains('active');
      
      // Close all first
      faqItems.forEach(i => i.classList.remove('active'));
      
      // If was not active, open it
      if (!isActive) {
        item.classList.add('active');
      }
    });
  });

  // Live Metrics Simulator
  const cpuProgress = document.getElementById('cpu-progress');
  const cpuText = document.getElementById('cpu-text');
  const ramProgress = document.getElementById('ram-progress');
  const ramText = document.getElementById('ram-text');
  const diskProgress = document.getElementById('disk-progress');
  const diskText = document.getElementById('disk-text');

  let baseDiskUsedPercent = 95.8; // starts very high

  function updateMetrics() {
    // CPU fluctuates between 12% and 38%
    const cpuVal = Math.floor(Math.random() * 26) + 12;
    if (cpuProgress && cpuText) {
      cpuProgress.style.width = `${cpuVal}%`;
      cpuText.textContent = `${cpuVal}%`;
    }

    // RAM fluctuates between 42% and 58%
    const ramVal = Math.floor(Math.random() * 16) + 42;
    if (ramProgress && ramText) {
      ramProgress.style.width = `${ramVal}%`;
      ramText.textContent = `${ramVal}%`;
    }

    // Disk updates based on simulation
    if (diskProgress && diskText) {
      diskProgress.style.width = `${baseDiskUsedPercent}%`;
      // Assuming 512GB total disk
      const freeGB = ((100 - baseDiskUsedPercent) / 100 * 512).toFixed(1);
      diskText.textContent = `${freeGB} GB Free`;
    }
  }

  // Initial call and set interval
  updateMetrics();
  setInterval(updateMetrics, 3000);

  // Mock Scan & Clean Simulation
  const mockScanBtn = document.getElementById('mock-scan-btn');
  const mockTargetList = document.getElementById('mock-target-list');
  const mockOverlay = document.getElementById('mock-overlay');
  const mockStatusText = document.getElementById('mock-status-text');
  const mockDescText = document.getElementById('mock-desc-text');
  
  const scanStages = [
    "Analyzing Node.js caches...",
    "Scanning Xcode DerivedData...",
    "Crawling Rust target directories...",
    "Calculating Docker images size...",
    "Measuring Flutter build folders...",
    "Finalizing cleanup report..."
  ];

  let currentSimulationState = 'idle'; // idle -> scanning -> review -> cleaning -> done

  if (mockScanBtn) {
    mockScanBtn.addEventListener('click', () => {
      if (currentSimulationState === 'idle') {
        startMockScan();
      } else if (currentSimulationState === 'review') {
        startMockClean();
      } else if (currentSimulationState === 'done') {
        resetMockDemo();
      }
    });
  }

  function startMockScan() {
    currentSimulationState = 'scanning';
    mockScanBtn.disabled = true;
    mockScanBtn.textContent = 'Scanning...';
    mockScanBtn.classList.add('scanning');
    
    // Hide list, show overlay
    mockTargetList.style.display = 'none';
    mockOverlay.style.display = 'flex';
    
    let stageIdx = 0;
    mockStatusText.textContent = scanStages[0];
    
    const stageInterval = setInterval(() => {
      stageIdx++;
      if (stageIdx < scanStages.length) {
        mockStatusText.textContent = scanStages[stageIdx];
      } else {
        clearInterval(stageInterval);
        showMockReview();
      }
    }, 800);
  }

  function showMockReview() {
    currentSimulationState = 'review';
    mockScanBtn.disabled = false;
    mockScanBtn.textContent = 'Clean Sweep (66.6 GB)';
    mockScanBtn.classList.remove('scanning');
    mockScanBtn.style.background = '#ef4444'; // Red for purge warning
    
    mockOverlay.style.display = 'none';
    mockTargetList.style.display = 'flex';
    
    // Populate targets with results
    mockTargetList.innerHTML = `
      <div class="mock-target-card fade-in">
        <div class="mock-target-info">
          <div class="mock-target-icon mobile">📱</div>
          <div class="mock-target-details">
            <h4>Xcode & iOS Cache</h4>
            <p>~/Library/Developer/Xcode/DerivedData</p>
          </div>
        </div>
        <div class="mock-target-action">
          <span class="mock-target-size">24.5 GB</span>
        </div>
      </div>
      
      <div class="mock-target-card fade-in">
        <div class="mock-target-info">
          <div class="mock-target-icon web">🌐</div>
          <div class="mock-target-details">
            <h4>Node.js Project Caches</h4>
            <p>/Users/developer/projects/**/node_modules</p>
          </div>
        </div>
        <div class="mock-target-action">
          <span class="mock-target-size">18.2 GB</span>
        </div>
      </div>
      
      <div class="mock-target-card fade-in">
        <div class="mock-target-info">
          <div class="mock-target-icon systems">🦀</div>
          <div class="mock-target-details">
            <h4>Rust Cargo Targets</h4>
            <p>/Users/developer/dev/**/target/debug</p>
          </div>
        </div>
        <div class="mock-target-action">
          <span class="mock-target-size">14.1 GB</span>
        </div>
      </div>
      
      <div class="mock-target-card fade-in">
        <div class="mock-target-info">
          <div class="mock-target-icon special">🐳</div>
          <div class="mock-target-details">
            <h4>Unused Docker Layers</h4>
            <p>Docker Engine cache volumes</p>
          </div>
        </div>
        <div class="mock-target-action">
          <span class="mock-target-size">9.8 GB</span>
        </div>
      </div>
    `;

    if (mockDescText) {
      mockDescText.textContent = "Scan complete. Review targets and clean.";
    }
  }

  function startMockClean() {
    currentSimulationState = 'cleaning';
    mockScanBtn.disabled = true;
    mockScanBtn.textContent = 'Purging files...';
    
    // Add deletion visual effect
    const cards = mockTargetList.querySelectorAll('.mock-target-card');
    cards.forEach((card, idx) => {
      setTimeout(() => {
        card.style.transform = 'scale(0.95)';
        card.style.opacity = '0.3';
        card.querySelector('.mock-target-size').textContent = 'Deleted';
        card.querySelector('.mock-target-size').style.color = '#ef4444';
      }, idx * 400);
    });

    setTimeout(() => {
      // Complete Clean
      currentSimulationState = 'done';
      mockScanBtn.disabled = false;
      mockScanBtn.textContent = 'Scan Again';
      mockScanBtn.style.background = 'var(--secondary)';
      
      mockTargetList.innerHTML = `
        <div style="text-align: center; padding: 3rem 0; width: 100%;" class="fade-in">
          <div style="font-size: 3rem; margin-bottom: 1rem;">🎉</div>
          <h4 style="font-size: 1.25rem; font-weight: 700; margin-bottom: 0.5rem; color: #10b981;">Clean Sweep Successful!</h4>
          <p style="color: var(--text-muted); font-size: 0.9rem;">You reclaimed 66.6 GB of valuable storage space.</p>
        </div>
      `;
      
      if (mockDescText) {
        mockDescText.textContent = "Disk optimizer successfully run.";
      }
      
      // Update disk usage in sidebar dynamically!
      // 66.6 GB out of 512 GB is about 13% of the disk. So decrease used disk percent.
      baseDiskUsedPercent = 82.8;
      updateMetrics();
    }, 1800);
  }

  function resetMockDemo() {
    currentSimulationState = 'idle';
    mockScanBtn.textContent = 'Global Scan';
    mockScanBtn.style.background = 'var(--secondary)';
    
    if (mockDescText) {
      mockDescText.textContent = "Select custom directory path to begin scan.";
    }
    
    mockTargetList.innerHTML = `
      <div class="mock-target-card">
        <div class="mock-target-info">
          <div class="mock-target-icon mobile">📱</div>
          <div class="mock-target-details">
            <h4>Mobile Environment Caches</h4>
            <p>Xcode, Android SDK, Flutter builds</p>
          </div>
        </div>
        <div class="mock-target-action">
          <span class="mock-target-size">Pending</span>
        </div>
      </div>
      <div class="mock-target-card">
        <div class="mock-target-info">
          <div class="mock-target-icon web">🌐</div>
          <div class="mock-target-details">
            <h4>Web Project Dependencies</h4>
            <p>node_modules, caches, packages</p>
          </div>
        </div>
        <div class="mock-target-action">
          <span class="mock-target-size">Pending</span>
        </div>
      </div>
      <div class="mock-target-card">
        <div class="mock-target-info">
          <div class="mock-target-icon systems">⚙️</div>
          <div class="mock-target-details">
            <h4>System & Compiler Artifacts</h4>
            <p>Rust targets, Python Conda, .NET, PHP</p>
          </div>
        </div>
        <div class="mock-target-action">
          <span class="mock-target-size">Pending</span>
        </div>
      </div>
    `;
    
    baseDiskUsedPercent = 95.8;
    updateMetrics();
  }
});
