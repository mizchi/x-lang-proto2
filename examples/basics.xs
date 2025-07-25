-- XS言語の基本機能

-- 算術演算
10 + 20                   -- => 30

-- 関数定義と適用
let add = fn x = fn y = x + y in add 3 4  -- => 7

-- let束縛
let x = 42 in x           -- => 42

-- 条件分岐
if 5 < 10 { "yes" } else { "no" }    -- => "yes"

-- リスト
cons 1 [2, 3, 4]          -- => [1, 2, 3, 4]