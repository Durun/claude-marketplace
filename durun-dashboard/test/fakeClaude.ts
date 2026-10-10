// テストの偽の claude は sh のスクリプトで、Windows の spawn は .exe しか探さないので実行できない。
// そのまま走らせると PATH の先にある本物の claude を呼んでしまうので、偽の claude に頼るテストは Windows では飛ばす。
export const skipFakeClaude = process.platform === 'win32' && '偽の claude は sh のスクリプトで、Windows では実行できない';
