import React from 'react'

const decodeEntities = (value: string): string => value
    .replace(/&nbsp;/gi, ' ')
    .replace(/&amp;/gi, '&')
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/&quot;/gi, '"')
    .replace(/&#39;/gi, "'")

const normalizeInlineMarkdown = (value: string): string => value
    .replace(/\*\*([^*]+)\*\*/g, '$1')
    .replace(/__([^_]+)__/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '$1 ($2)')

const toPlainLines = (notes: string): string[] => decodeEntities(notes
    .replace(/\r\n/g, '\n')
    .replace(/<br\s*\/?>/gi, '\n')
    .replace(/<h([1-6])[^>]*>([\s\S]*?)<\/h\1>/gi, (_, level, inner) => `\n${'#'.repeat(Number(level))} ${inner}\n`)
    .replace(/<li[^>]*>([\s\S]*?)<\/li>/gi, (_, inner) => `\n- ${inner}\n`)
    .replace(/<\/p>/gi, '\n')
    .replace(/<[^>]*>/g, ''))
    .split('\n')

export const renderReleaseNotes = (notes: string): React.ReactNode => {
    const blocks: React.ReactNode[] = []
    let paragraph: string[] = []
    let bullets: string[] = []

    const flushParagraph = () => {
        if (paragraph.length === 0) return
        blocks.push(<p key={`p-${blocks.length}`}>{paragraph.join(' ')}</p>)
        paragraph = []
    }
    const flushBullets = () => {
        if (bullets.length === 0) return
        blocks.push(
            <ul key={`ul-${blocks.length}`}>
                {bullets.map((item, index) => <li key={index}>{item}</li>)}
            </ul>
        )
        bullets = []
    }

    for (const rawLine of toPlainLines(notes)) {
        const line = rawLine.trim()
        if (!line) {
            flushParagraph()
            flushBullets()
            continue
        }
        const heading = line.match(/^(#{1,6})\s+(.*)$/)
        if (heading) {
            flushParagraph()
            flushBullets()
            const tag = `h${Math.min(heading[1].length + 1, 6)}` as keyof React.JSX.IntrinsicElements
            blocks.push(React.createElement(tag, { key: `h-${blocks.length}` }, normalizeInlineMarkdown(heading[2])))
            continue
        }
        const bullet = line.match(/^[-*]\s+(.*)$/)
        if (bullet) {
            flushParagraph()
            bullets.push(normalizeInlineMarkdown(bullet[1]))
            continue
        }
        flushBullets()
        paragraph.push(normalizeInlineMarkdown(line))
    }

    flushParagraph()
    flushBullets()
    return blocks
}
