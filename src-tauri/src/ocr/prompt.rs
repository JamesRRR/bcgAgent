pub const PROMPT: &str =
    "你是一位桌游规则书 OCR 与版面解析专家。请将这一页转写为结构化 Markdown：\n\
- 保留标题层级（#、##、###）\n\
- 保留有序列表和无序列表\n\
- 表格用 Markdown 表格语法\n\
- 图标 / 插图用 `![icon: 简短描述]` 描述\n\
- 中英文照原样保留，不要翻译\n\
仅输出 Markdown 内容本身，不要解释、不要使用 ``` 包裹。";

pub const GROUNDED_PROMPT: &str = "你是一位桌游规则书 OCR 与版面解析专家。\n\
请同时输出两部分：\n\
1) 这一页的结构化 Markdown 转写。\n\
2) 这一页中所有插图 / 示意图 / 卡牌图 / 图标的边界框（不要包括纯文本段落、标题、页码或表格）。\n\n\
严格按以下 JSON 格式返回，不要附加任何解释、不要使用 ``` 包裹：\n\
{\n\
  \"markdown\": \"<完整 Markdown 转写>\",\n\
  \"illustrations\": [\n\
    {\"bbox_2d\": [x1, y1, x2, y2], \"label\": \"<简短中文描述>\"}\n\
  ]\n\
}\n\n\
要求：\n\
- bbox_2d 使用输入图像的绝对像素坐标，左上为原点，[x1, y1] 是左上角，[x2, y2] 是右下角。\n\
- 如果本页没有任何插图，illustrations 返回空数组 []。\n\
- **关键**：在 Markdown 中，每张插图所在位置必须用 `![label](ill:N)` 占位，N 从 0 开始递增、与上面 illustrations 数组中的下标一一对应。括号里的 N 不能省，也不能跳号。例如：第一张插图 `![羽毛栏](ill:0)`，第二张 `![卡牌效果](ill:1)`。\n\
- 占位应当出现在它在原页面里出现的位置（贴近它解释的文字），而不是统一堆在末尾。\n\
- 中英文照原样保留，不要翻译。";

/// Caption prompt for the per-illustration second pass. Input is a tightly
/// cropped image of a single illustration; output is a 1-2 句中文 description
/// useful for RAG and for the walkthrough coach to answer "long what 样" type
/// questions.
pub const CAPTION_PROMPT: &str = "你是一位桌游卡牌与图标视觉解读专家。\n\
你看到的是一张桌游规则书中的插图、卡牌、图标或示意图的特写。\n\
请用 1-2 句简洁中文描述：\n\
- 画面里有什么（颜色、构图、主要元素）。\n\
- 它在游戏中的含义/作用，如果可以从图本身推断出来。\n\
仅输出描述文字本身，不要使用引号、不要使用 markdown、不要解释、不要使用 ``` 包裹。";
