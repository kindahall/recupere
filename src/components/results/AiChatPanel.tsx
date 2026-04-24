import { Bot, Loader, Send, User } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { type GemmaStatus, chatWithAi, fetchGemmaStatus } from '../../hooks/useIpc';
import { WarningBanner } from '../common/WarningBanner';

interface Message {
  role: 'user' | 'assistant';
  content: string;
  timestamp: number;
}

interface AiChatPanelProps {
  scanId: string;
}

export function AiChatPanel({ scanId }: AiChatPanelProps) {
  const { t } = useTranslation();
  const [messages, setMessages] = useState<Message[]>([
    {
      role: 'assistant',
      content: t('chat.welcome'),
      timestamp: Date.now(),
    },
  ]);
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const [gemmaStatus, setGemmaStatus] = useState<GemmaStatus | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (messages.length === 0) {
      return;
    }
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  useEffect(() => {
    let cancelled = false;
    fetchGemmaStatus()
      .then((s) => {
        if (!cancelled) setGemmaStatus(s);
      })
      .catch(() => {
        if (!cancelled) setGemmaStatus(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const gemmaReady = gemmaStatus?.ready ?? false;

  const sendMessage = async () => {
    const text = input.trim();
    if (!text || loading) return;

    const userMsg: Message = { role: 'user', content: text, timestamp: Date.now() };
    setMessages((prev) => [...prev, userMsg]);
    setInput('');
    setLoading(true);

    try {
      const response = await chatWithAi(scanId, text);
      const aiMsg: Message = { role: 'assistant', content: response, timestamp: Date.now() };
      setMessages((prev) => [...prev, aiMsg]);
    } catch (err) {
      const errorMsg: Message = {
        role: 'assistant',
        content: t('chat.error_prefix', {
          error: err instanceof Error ? err.message : String(err),
        }),
        timestamp: Date.now(),
      };
      setMessages((prev) => [...prev, errorMsg]);
    } finally {
      setLoading(false);
      inputRef.current?.focus();
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  };

  return (
    <div className="ai-chat-panel">
      <div className="ai-chat-header">
        <Bot size={18} style={{ color: 'var(--color-accent)' }} />
        <span className="font-medium">{t('chat.title')}</span>
      </div>

      <div className="ai-chat-messages">
        {messages.map((msg, i) => (
          <div key={i} className={`ai-chat-message ${msg.role}`}>
            <div className="ai-chat-avatar">
              {msg.role === 'assistant' ? <Bot size={14} /> : <User size={14} />}
            </div>
            <div className="ai-chat-bubble">{msg.content}</div>
          </div>
        ))}
        {loading && (
          <div className="ai-chat-message assistant">
            <div className="ai-chat-avatar">
              <Bot size={14} />
            </div>
            <div className="ai-chat-bubble">
              <Loader size={14} className="ai-chat-loading" />
              {t('chat.thinking')}
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      {!gemmaReady && gemmaStatus && (
        <div style={{ padding: '8px 12px' }}>
          <WarningBanner variant="warning">
            {t('settings.gemma_unavailable_banner')} {gemmaStatus.message}
          </WarningBanner>
        </div>
      )}

      <div className="ai-chat-input-bar">
        <input
          ref={inputRef}
          type="text"
          className="ai-chat-input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={t('chat.placeholder')}
          disabled={loading || !gemmaReady}
        />
        <button
          type="button"
          className="btn btn-primary btn-icon btn-sm"
          onClick={sendMessage}
          disabled={loading || !input.trim() || !gemmaReady}
        >
          <Send size={14} />
        </button>
      </div>
    </div>
  );
}
