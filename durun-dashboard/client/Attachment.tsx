// PR や Issue に GitHub で添付した画像や動画を、ダッシュボードのウィンドウに浮かべて見せる。
import { VideoPlayer } from './Docs.tsx';
import { type Float, FloatFrame, dock, floatUp } from './Float.tsx';

/** ウィンドウに浮かべた添付の保存キー。タブの id に添付の URL を入れ、settings に題名と、動画なら video を持つ。 */
export const ATTACHMENT_KEY = 'attachment';

/** 添付をサーバー経由で読む URL。非公開のリポジトリの添付は、画面から GitHub を直接読めないため。 */
export const attachmentSrc = (url: string): string => `/api/github/attachment?${new URLSearchParams({ url })}`;

export const openAttachment = (url: string, title: string, video: boolean): void =>
  floatUp(ATTACHMENT_KEY, '', { id: url, settings: video ? { title, video: '1' } : { title } });

export const AttachmentFloat = ({ f }: { f: Float }) => {
  const title = f.tab.settings?.title || '添付';
  const video = !!f.tab.settings?.video;
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  return (
    <FloatFrame f={f} title={title} icon={video ? '🎬' : '🖼️'} controls={[close]}>
      {video ? <VideoPlayer src={attachmentSrc(f.tab.id)} name={title} /> : <img class="file-media" src={attachmentSrc(f.tab.id)} alt={title} />}
    </FloatFrame>
  );
};
