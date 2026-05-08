import { Info, Globe, Mail, Github, RefreshCw, CheckCircle2, ShieldCheck } from 'lucide-react';
import { motion } from 'motion/react';
import { useState } from 'react';

export default function AboutView() {
  const [checking, setChecking] = useState(false);
  const [lastCheck, setLastCheck] = useState('2026-05-08 16:45');

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
        <div className="w-20 h-20 bg-primary/10 border border-primary/20 rounded-2xl flex items-center justify-center mx-auto shadow-2xl">
          <ShieldCheck className="w-10 h-10 text-primary" />
        </div>
        <div className="space-y-1">
          <h2 className="text-3xl font-bold tracking-tighter">DevClean Orchestrator</h2>
          <p className="text-on-surface/40 font-mono text-xs uppercase tracking-[0.3em]">Version 1.0.4-stable</p>
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
                <p className="text-[10px] text-on-surface/40 font-mono">Principal Systems Architect</p>
              </div>
            </div>

            <div className="space-y-2 pt-2">
              <a 
                href="https://www.abdulrasol.github.io" 
                target="_blank" 
                rel="noreferrer"
                className="flex items-center gap-3 p-3 rounded-lg hover:bg-surface-bright transition-all group"
              >
                <div className="p-1.5 bg-surface-dim border border-outline rounded group-hover:border-primary/50">
                  <Globe className="w-3.5 h-3.5 text-on-surface/40 group-hover:text-primary" />
                </div>
                <span className="text-xs text-on-surface/60 group-hover:text-on-surface">www.abdulrasol.github.io</span>
              </a>
              
              <a 
                href="mailto:abdulrsol97@gmail.com"
                className="flex items-center gap-3 p-3 rounded-lg hover:bg-surface-bright transition-all group"
              >
                <div className="p-1.5 bg-surface-dim border border-outline rounded group-hover:border-primary/50">
                  <Mail className="w-3.5 h-3.5 text-on-surface/40 group-hover:text-primary" />
                </div>
                <span className="text-xs text-on-surface/60 group-hover:text-on-surface">abdulrsol97@gmail.com</span>
              </a>
            </div>
          </div>
        </section>
      </div>

      <footer className="text-center pt-8 border-t border-outline">
        <p className="text-[10px] text-on-surface/20 uppercase tracking-[0.5em] font-mono">
          Made with Precision in Iraq • 2026
        </p>
      </footer>
    </div>
  );
}
