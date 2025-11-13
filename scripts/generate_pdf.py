#!/usr/bin/env python3
"""
Simple Markdown to PDF generator using reportlab for this workspace.
This script is only used to create the requested PDF documentation file from the markdown.
"""
import sys
from pathlib import Path


try:
    from reportlab.lib.pagesizes import A4
    from reportlab.lib.styles import getSampleStyleSheet
    from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer
    from reportlab.lib.units import mm
except Exception:
    print("Missing reportlab. Please install with: pip install reportlab")
    raise

MARKDOWN_PATH = Path(__file__).resolve().parents[1] / 'DOCUMENTATION_BLOCKCHAIN_CORE_CHANGES.md'
PDF_PATH = Path(__file__).resolve().parents[1] / 'blockchain_core_changes.pdf'

def markdown_to_paragraphs(md_text):
    """Very small markdown -> paragraphs converter: handles headers and paragraphs."""
    lines = [l.rstrip() for l in md_text.splitlines()]
    paras = []
    cur = []

    def flush():
        if cur:
            paras.append('\n'.join(cur))
            cur.clear()

    for line in lines:
        if not line.strip():
            flush()
            continue
        if line.startswith('#'):
            flush()
            # represent header as paragraph with bold style later
            paras.append(line)
        else:
            cur.append(line)
    flush()
    return paras


def build_pdf():
    md = MARKDOWN_PATH.read_text(encoding='utf-8')
    doc = SimpleDocTemplate(str(PDF_PATH), pagesize=A4,
                            rightMargin=20*mm, leftMargin=20*mm,
                            topMargin=20*mm, bottomMargin=20*mm)
    styles = getSampleStyleSheet()
    story = []
    for p in markdown_to_paragraphs(md):
        if p.startswith('Title:'):
            txt = p.replace('Title:','').strip()
            story.append(Paragraph(txt, styles['Title']))
            story.append(Spacer(1,6))
            continue
        if p.startswith('#'):
            hlevel = p.count('#')
            txt = p.lstrip('#').strip()
            style = 'Heading%s' % min(3, hlevel)
            if style not in styles:
                style = 'Heading3'
            story.append(Paragraph(txt, styles[style]))
            story.append(Spacer(1,4))
            continue
        # regular paragraph
        story.append(Paragraph(p.replace('&', '&amp;').replace('<','&lt;'), styles['BodyText']))
        story.append(Spacer(1,4))

    doc.build(story)
    print(f"Wrote PDF to: {PDF_PATH}")

if __name__ == '__main__':
    build_pdf()
