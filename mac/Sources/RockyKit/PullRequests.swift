// 메뉴에 그릴 목록 — 순수 함수.
import Foundation

/// 메뉴의 "머지 후보" — 열린 PR 중 데몬이 ready 로 판정한 것. 머지는 사람 몫이고, 알림(배너)은 데몬이 이미 띄운다.
public func readyPrs(_ prs: [PrSnapshot]) -> [PrSnapshot] {
    prs.filter { $0.state == "OPEN" && $0.ready && !$0.isDraft }
}
