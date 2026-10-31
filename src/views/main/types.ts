import type { CardStatus } from '../../components/CardStatus';
import type { StatusNotice } from '../../fluent-controls/status';
import type * as dict from '../../logics/dict';

/** 主界面的三本词典 */
export type DictName = 'collins' | 'oxford' | 'youdao';

/** 单词列表的单词卡片 */
export interface ItemModel<T> {
    item: T;
    status: CardStatus;
    id: number | null;
    /** 该卡片自己的就地反馈（连接进度、编辑降级提示等） */
    notice?: StatusNotice | null;
}

/** 每本字典中的搜索结果 */
export interface WordItems {
    collins: ItemModel<dict.CollinsItem>[];
    oxford: ItemModel<dict.OxfordItem>[];
    youdao: ItemModel<dict.YoudaoItem>[];
}

/** 划词面板的词元 */
export interface SentenceToken {
    token: string;
    marked: boolean;
}
