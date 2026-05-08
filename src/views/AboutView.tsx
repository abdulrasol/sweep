import { Globe, Mail, Github, RefreshCw, CheckCircle2, Zap } from 'lucide-react';
import { motion } from 'motion/react';
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

export default function AboutView() {
  const [checking, setChecking] = useState(false);
  const [lastCheck, setLastCheck] = useState('2026-05-08 16:45');
  const [version, setVersion] = useState('0.0.0');

  useEffect(() => {
    const fetchVersion = async () => {
      try {
        const v = await invoke<string>('get_app_version');
        setVersion(v);
      } catch (err) {
        console.error("Failed to fetch version:", err);
      }
    };
    fetchVersion();
  }, []);

  const handleCheckUpdate = () => {
    setChecking(true);
    setTimeout(() => {
      setChecking(false);
      setLastCheck(new Date().toLocaleString());
    }, 2000);
  };

  return (
    <div className="max-w-4xl mx-auto py-12 px-6 space-y-10">
      {/* Brand Section */}
      <section className="text-center space-y-4">
        <div className="w-24 h-24 rounded-3xl overflow-hidden border-2 border-primary/40 mx-auto shadow-[0_0_30px_rgba(var(--color-primary-rgb),0.15)] bg-surface-container relative">
          <img 
            src="/sweep.png" 
            alt="Sweep Logo" 
            className="w-full h-full object-cover"
            referrerPolicy="no-referrer"
          />
        </div>
        <div className="space-y-1">
          <h2 className="text-3xl font-bold tracking-tighter text-on-surface">Sweep Orchestrator</h2>
          <p className="text-primary font-mono text-xs uppercase tracking-[0.3em]">RECLAIM YOUR STORAGE</p>
        </div>
      </section>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Update Section */}
        <section className="bg-surface-container border border-outline rounded-2xl p-6 space-y-6">
          <div className="flex items-center gap-3">
            <RefreshCw className="w-4 h-4 text-primary" />
            <h3 className="text-xs font-bold uppercase tracking-widest text-on-surface/60">Software Update</h3>
          </div>
          
          <div className="p-4 bg-surface-dim border border-outline rounded-xl space-y-3">
            <div className="flex items-center justify-between">
              <span className="text-xs text-on-surface/40">Status:</span>
              <span className="text-xs font-bold text-primary flex items-center gap-1">
                <CheckCircle2 className="w-3 h-3" /> Up to date
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-xs text-on-surface/40">Current Version:</span>
              <span className="text-xs font-mono text-on-surface/60">v{version}</span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-xs text-on-surface/40">Last Checked:</span>
              <span className="text-xs font-mono text-on-surface/60">{lastCheck}</span>
            </div>
          </div>

          <button 
            onClick={handleCheckUpdate}
            disabled={checking}
            className="w-full py-3 bg-surface-bright border border-outline rounded-xl text-xs font-bold uppercase tracking-widest hover:bg-surface-bright/80 transition-all flex items-center justify-center gap-3"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${checking ? 'animate-spin' : ''}`} />
            {checking ? 'Checking Servers...' : 'Check for Updates'}
          </button>
        </section>

        {/* Developer Section */}
        <section className="bg-surface-container border border-outline rounded-2xl p-6 space-y-6">
          <div className="flex items-center gap-3">
            <Globe className="w-4 h-4 text-primary" />
            <h3 className="text-xs font-bold uppercase tracking-widest text-on-surface/60">Developer Profile</h3>
          </div>

          <div className="space-y-4">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-full bg-primary/20 border border-primary/30 flex items-center justify-center text-primary font-bold text-lg">
                AR
              </div>
              <div>
                <h4 className="font-bold text-on-surface">Abdulrasol Al-Hilo</h4>
                <p className="text-[10px] text-on-surface/40 font-mono">Flutter/Dart, AI, Vibe & More</p>
              </div>
            </div>

            <div className="space-y-2 pt-2">
              <a 
                href="https://abdulrasol.github.io" 
                target="_blank" 
                rel="noreferrer"
                className="flex items-center gap-3 p-3 rounded-lg hover:bg-surface-bright transition-all group"
              >
                <div className="p-1.5 bg-surface-dim border border-outline rounded group-hover:border-primary/50">
                  <Globe className="w-3.5 h-3.5 text-on-surface/40 group-hover:text-primary" />
                </div>
                <span className="text-xs text-on-surface/60 group-hover:text-on-surface">website</span>
                <span className="text-xs text-on-surface/60 group-hover:text-on-surface">abdulrasol.github.io</span>
              </a>
              
              <a 
                href="https://github.com/abdulrasol/" 
                target="_blank" 
                rel="noreferrer"
                className="flex items-center gap-3 p-3 rounded-lg hover:bg-surface-bright transition-all group"
              >
                <div className="p-1.5 bg-surface-dim border border-outline rounded group-hover:border-primary/50">
                  <Github className="w-3.5 h-3.5 text-on-surface/40 group-hover:text-primary" />
                </div>
                <span className="text-xs text-on-surface/60 group-hover:text-on-surface">github</span>
                <span className="text-xs text-on-surface/60 group-hover:text-on-surface">github.com/abdulrasol</span>
              </a>

              <a 
                href="mailto:abdulrsol97@gmail.com"
                className="flex items-center gap-3 p-3 rounded-lg hover:bg-surface-bright transition-all group"
              >
                <div className="p-1.5 bg-surface-dim border border-outline rounded group-hover:border-primary/50">
                  <Mail className="w-3.5 h-3.5 text-on-surface/40 group-hover:text-primary" />
                </div>
                <span className="text-xs text-on-surface/60 group-hover:text-on-surface">email</span>
                <span className="text-xs text-on-surface/60 group-hover:text-on-surface">abdulrsol97@gmail.com</span>
              </a>
            </div>
          </div>
        </section>
      </div>

      {/* Origin Story Section */}
      <section className="bg-surface-container border border-outline rounded-2xl p-8 space-y-6 relative overflow-hidden">
        <div className="absolute top-0 right-0 p-8 opacity-[0.03] pointer-events-none">
          <Zap className="w-32 h-32 text-primary" />
        </div>
        
        <div className="flex items-center gap-3 relative">
          <div className="p-2 bg-primary/10 rounded-lg">
            <Zap className="w-4 h-4 text-primary" />
          </div>
          <h3 className="text-xs font-bold uppercase tracking-widest text-on-surface/60">The "1GB Incident" Origin</h3>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-10 items-center relative">
          <div className="space-y-4 text-sm text-on-surface/60 leading-relaxed">
            <p>
              Sweep was born in a moment of pure developer frustration. Yesterday at work, a classmate asked me to run a Flutter app on his device. I ran <span className="font-mono text-[10px] bg-surface-dim px-1.5 py-0.5 rounded border border-outline/30 text-primary">flutter run ios</span>, but it crashed.
            </p>
            <p>
              We assumed it was his iPhone storage. He had 100GB free. I checked my Mac and realized I had exactly **1 GB of space left**.
            </p>
            <p>
              Google Antigravity (running on the cloud free model) guided me to the hidden artifacts eating my disk. I realized every dev needs a better way to clean their workspace.
            </p>
          </div>
          
          <div className="bg-surface-dim/50 border border-outline/30 p-6 rounded-xl space-y-4">
            <div className="flex items-center gap-2">
              <div className="w-2 h-2 rounded-full bg-primary animate-pulse" />
              <span className="text-[10px] font-bold uppercase tracking-widest text-on-surface/40 text-primary">AI Collaboration</span>
            </div>
            <p className="text-xs text-on-surface/50 italic leading-relaxed">
              "This entire application was architected and built using Vide Coding techniques with **Google Gemini**. When cloud limits hit, we pushed through using **Gemini 3.5 Flash** to maintain the high-performance momentum."
            </p>
          </div>
        </div>
      </section>

      <footer className="text-center pt-8 border-t border-outline space-y-2">
        <p className="text-[10px] text-on-surface/20 uppercase tracking-[0.5em] font-mono">
          Made with Precision in Iraq • 2026
        </p>
        <p className="text-[9px] text-primary/30 uppercase font-bold tracking-[0.2em]">
          Co-Authored by Google Gemini (Antigravity)
        </p>
      </footer>
    </div>
  );
}
