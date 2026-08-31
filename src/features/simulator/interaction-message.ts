import hljs from 'highlight.js/lib/core';
import xmlLanguage from 'highlight.js/lib/languages/xml';

hljs.registerLanguage('xml', xmlLanguage);

export const xmlHighlighter = hljs;

export const MAX_FORMATTABLE_MESSAGE_LENGTH = 1024 * 1024;

export type InteractionMessageView = {
  headers: string;
  body: string;
  formattedBody: string | null;
  isXml: boolean;
  formatted: boolean;
  error?: string;
};

function splitSipMessage(message: string): { headers: string; body: string } | null {
  const separator = message.match(/\r?\n\r?\n/);
  if (separator === null || separator.index === undefined) {
    return null;
  }

  return {
    headers: message.slice(0, separator.index),
    body: message.slice(separator.index + separator[0].length),
  };
}

function hasXmlContentType(headers: string): boolean {
  return /^Content-Type\s*:\s*[^\r\n]*xml/i.test(headers);
}

function looksLikeXml(body: string, headers: string): boolean {
  const candidate = body.trimStart();
  return (
    hasXmlContentType(headers) ||
    /^<\?xml(?:\s|\?>)/i.test(candidate) ||
    /^<(?:Notify|Response|Control|Query|CmdType)\b/i.test(candidate)
  );
}

function serializeNode(node: Node): string {
  return new XMLSerializer().serializeToString(node);
}

function formatElement(element: Element, depth: number): string {
  const serialized = serializeNode(element);
  const openingEnd = serialized.indexOf('>');
  if (openingEnd < 0) {
    return `${'  '.repeat(depth)}${serialized}`;
  }

  const opening = serialized.slice(0, openingEnd + 1);
  if (opening.endsWith('/>')) {
    return `${'  '.repeat(depth)}${opening}`;
  }

  const closing = `</${element.tagName}>`;
  const children = [...element.childNodes].filter(
    (child) => child.nodeType !== Node.TEXT_NODE || (child.nodeValue?.trim() ?? '').length > 0,
  );
  if (children.length === 0) {
    return `${'  '.repeat(depth)}${opening}${closing}`;
  }

  const hasOnlyText = children.every((child) => child.nodeType === Node.TEXT_NODE);
  if (hasOnlyText) {
    const text = children.map((child) => serializeNode(child).trim()).join('');
    return `${'  '.repeat(depth)}${opening}${text}${closing}`;
  }

  const childLines = children.map((child) => {
    if (child.nodeType === Node.ELEMENT_NODE) {
      return formatElement(child as Element, depth + 1);
    }
    return `${'  '.repeat(depth + 1)}${serializeNode(child).trim()}`;
  });
  return `${'  '.repeat(depth)}${opening}\n${childLines.join('\n')}\n${'  '.repeat(depth)}${closing}`;
}

function prettyPrintXml(document: Document, declaration: string | undefined): string {
  const lines: string[] = [];
  if (declaration !== undefined) {
    lines.push(declaration);
  }
  if (document.documentElement !== null) {
    lines.push(formatElement(document.documentElement, 0));
  }
  return lines.join('\n');
}

function formatXmlBody(body: string): { formattedBody: string } | null {
  const document = new DOMParser().parseFromString(body, 'application/xml');
  if (
    document.documentElement === null ||
    document.getElementsByTagName('parsererror').length > 0
  ) {
    return null;
  }

  const declaration = body.match(/^\s*(<\?xml[\s\S]*?\?>)/i)?.[1];
  const formattedBody = prettyPrintXml(document, declaration);
  return { formattedBody };
}

/**
 * 将交互日志拆分为 SIP Header 和 XML Body，仅生成展示派生数据，不修改原始报文。
 */
export function formatInteractionMessage(message: string): InteractionMessageView {
  const parts = splitSipMessage(message);
  if (parts === null && !/^\s*<\?xml|^\s*<(?:Notify|Response|Control|Query)\b/i.test(message)) {
    return {
      headers: '',
      body: message,
      formattedBody: null,
      isXml: false,
      formatted: false,
    };
  }

  const headers = parts?.headers ?? '';
  const body = parts?.body ?? message;
  if (body.trim().length === 0 || !looksLikeXml(body, headers)) {
    return {
      headers,
      body,
      formattedBody: null,
      isXml: false,
      formatted: false,
    };
  }

  if (message.length > MAX_FORMATTABLE_MESSAGE_LENGTH) {
    return {
      headers,
      body,
      formattedBody: null,
      isXml: true,
      formatted: false,
      error: '消息超过 1 MB 展示限制，已保留原始内容。',
    };
  }

  const formatted = formatXmlBody(body);
  if (formatted === null) {
    return {
      headers,
      body,
      formattedBody: null,
      isXml: true,
      formatted: false,
      error: 'XML 解析失败，已保留原始内容。',
    };
  }

  return {
    headers,
    body,
    formattedBody: formatted.formattedBody,
    isXml: true,
    formatted: true,
  };
}
