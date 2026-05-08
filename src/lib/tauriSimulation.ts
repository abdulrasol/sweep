import { useState, useEffect } from 'react';

// Mock Tauri API for browser preview
// In a real Tauri app, these would call Rust commands
export const invoke = async (cmd: string, args?: any): Promise<any> => {
  console.log(`[Tauri Mock] Invoking: ${cmd}`, args);
  
  // Simulate network/processing delay
  await new Promise(resolve => setTimeout(resolve, 500));

  switch (cmd) {
    case 'get_default_path':
      return '/Users/developer/workspace/projects';
    case 'scan_environment':
      return [
        { id: '1', path: '~/projects/frontend-app/node_modules', type: 'Dependency', size: '1.2 GB', status: 'SAFE' },
        { id: '2', path: '~/projects/api-service/target', type: 'Build Artifact', size: '850 MB', status: 'SAFE' },
        { id: '3', path: '~/.cache/docker', type: 'System Cache', size: '8.4 GB', status: 'REVIEW' },
        { id: '4', path: '~/projects/legacy-site/.next', type: 'Build Cache', size: '420 MB', status: 'SAFE' },
        { id: '5', path: '/var/log/system-journals', type: 'Log Files', size: '3.3 GB', status: 'UNSAFE' },
      ];
    case 'start_cleaning':
      return { success: true, jobId: 'job_' + Math.random().toString(36).substr(2, 9) };
    default:
      return null;
  }
};

export const useScanningSimulation = (active: boolean) => {
  const [progress, setProgress] = useState(0);
  const [logs, setLogs] = useState<string[]>([]);
  const [freed, setFreed] = useState(0);

  useEffect(() => {
    if (!active) return;

    const messages = [
      "Initializing cleanup sequence ...",
      "Scanning targets matching criteria.",
      "Directory /users/dev/docker/volumes/temp-data requires elevated privileges. Attempting sudo override.",
      "Sudo override successful. Purging directory.",
      "Reclaimed 2.1GB from /users/dev/docker/volumes/temp-data.",
      "Targeting node_modules in /var/www/project-alpha. Thread 1 spawned.",
      "Targeting .next/cache in /users/dev/workspace/frontend-v2. Thread 2 spawned.",
    ];

    let currentMsg = 0;
    const interval = setInterval(() => {
      setProgress(prev => {
        if (prev >= 100) {
          clearInterval(interval);
          return 100;
        }
        
        // Randomly add a log message
        if (Math.random() > 0.7 && currentMsg < messages.length) {
          setLogs(prevLogs => [...prevLogs, `[${new Date().toLocaleTimeString()}] ${messages[currentMsg++]}`]);
        }

        return prev + Math.random() * 5;
      });
      
      setFreed(prev => prev + Math.random() * 0.1);
    }, 200);

    return () => clearInterval(interval);
  }, [active]);

  return { progress, logs, freed };
};
