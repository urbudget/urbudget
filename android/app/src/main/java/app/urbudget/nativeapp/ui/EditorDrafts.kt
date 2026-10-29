package app.urbudget.nativeapp.ui

import androidx.compose.runtime.*
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.EditorCheckpoint
import org.json.JSONObject

/** Explicit financial fields only. Passwords and account-authentication forms never enter here. */
@Composable
internal fun retainEditorDraft(vm: BudgetViewModel, target: String, baseline: JSONObject, values: JSONObject) {
    val identity = remember { vm.state.value.budgetIdentity }
    if (identity == null) return
    val failures = remember { vm.operationFailureEpoch }
    val checkpoint = remember { EditorCheckpoint(identity, target, EditorCheckpoint.editorBaseline(baseline, values).put("revision", vm.state.value.budgetRevision), vm::notifyError) }
    SideEffect { checkpoint.save(values) }
    DisposableEffect(checkpoint) {
        onDispose {
            // A legacy form may close just after queuing Save. Check its outcome on the same queue.
            vm.afterPending {
                if (vm.state.value.budgetIdentity == identity && vm.operationFailureEpoch == failures) checkpoint.discard(onlyIfActive = true)
            }
        }
    }
}
