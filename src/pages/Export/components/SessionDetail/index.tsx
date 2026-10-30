import React, { memo, useEffect } from 'react'
import type { SessionRow } from '../../types'
import { useSessionMetrics } from '../../hooks/useSessionMetrics'
import { formatAbsoluteDate } from '../../utils/format'
import { MessageSquare, Image, Mic, Video, FileText, Smile } from 'lucide-react'
import './SessionDetail.scss'

const DEFAULT_AVATAR =
  "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'%3E%3Crect width='64' height='64' fill='%23d8d8d8'/%3E%3Ccircle cx='32' cy='24' r='11' fill='%23f4f4f4'/%3E%3Cpath d='M12 60c2-13 10-20 20-20s18 7 20 20z' fill='%23f4f4f4'/%3E%3C/svg%3E"

interface SessionDetailProps {
  session: SessionRow
  onExportContent: (contentType: 'text' | 'image' | 'voice' | 'video' | 'file' | 'emoji') => void
}

const SessionDetail: React.FC<SessionDetailProps> = ({ session, onExportContent }) => {
  const { metricsMap, fetchMetrics, isLoading } = useSessionMetrics()

  useEffect(() => {
    fetchMetrics([session.username])
  }, [session.username, fetchMetrics])

  const metrics = metricsMap[session.username]

  return (
    <div className="session-detail">
      <div className="sd-header">
        <div className="sd-avatar">
          <img src={session.avatarUrl || DEFAULT_AVATAR} alt="avatar" />
        </div>
        <div className="sd-info">
          <h2 className="sd-name">{session.displayName}</h2>
          {session.remark && session.nickname && session.remark !== session.nickname && (
            <span className="sd-alias">昵称: {session.nickname}</span>
          )}
          <span className="sd-time">
            最后活跃: {session.lastTimestamp ? formatAbsoluteDate(session.lastTimestamp) : '未知'}
          </span>
        </div>
      </div>

      <div className="sd-stats-grid">
        <div className="stat-card" onClick={() => onExportContent('text')}>
          <div className="stat-icon message"><MessageSquare size={18} /></div>
          <div className="stat-content">
            <span className="stat-label">文本/总消息</span>
            <span className="stat-value">{metrics?.totalMessages ?? (isLoading ? '...' : 0)}</span>
          </div>
        </div>
        <div className="stat-card" onClick={() => onExportContent('image')}>
          <div className="stat-icon image"><Image size={18} /></div>
          <div className="stat-content">
            <span className="stat-label">图片</span>
            <span className="stat-value">{metrics?.imageMessages ?? (isLoading ? '...' : 0)}</span>
          </div>
        </div>
        <div className="stat-card" onClick={() => onExportContent('voice')}>
          <div className="stat-icon voice"><Mic size={18} /></div>
          <div className="stat-content">
            <span className="stat-label">语音</span>
            <span className="stat-value">{metrics?.voiceMessages ?? (isLoading ? '...' : 0)}</span>
          </div>
        </div>
        <div className="stat-card" onClick={() => onExportContent('video')}>
          <div className="stat-icon video"><Video size={18} /></div>
          <div className="stat-content">
            <span className="stat-label">视频</span>
            <span className="stat-value">{metrics?.videoMessages ?? (isLoading ? '...' : 0)}</span>
          </div>
        </div>
        <div className="stat-card" onClick={() => onExportContent('file')}>
          <div className="stat-icon file"><FileText size={18} /></div>
          <div className="stat-content">
            <span className="stat-label">文件</span>
            <span className="stat-value">{metrics?.fileMessages ?? (isLoading ? '...' : 0)}</span>
          </div>
        </div>
        <div className="stat-card" onClick={() => onExportContent('emoji')}>
          <div className="stat-icon emoji"><Smile size={18} /></div>
          <div className="stat-content">
            <span className="stat-label">表情</span>
            <span className="stat-value">{metrics?.emojiMessages ?? (isLoading ? '...' : 0)}</span>
          </div>
        </div>
      </div>
    </div>
  )
}

export default memo(SessionDetail)
