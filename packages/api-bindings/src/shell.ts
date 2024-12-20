import {
  NotificationType,
  ProcessChangedNotification,
  ShellPromptReturnedNotification,
  TextUpdate,
  HistoryUpdatedNotification,
  TextUpdateSchema,
} from "@aws/amazon-q-developer-cli-proto/fig";
import { sendInsertTextRequest } from "./requests.js";
import { _subscribe, NotificationResponse } from "./notifications.js";
import { create } from "@bufbuild/protobuf";

export const processDidChange = {
  subscribe(
    handler: (
      notification: ProcessChangedNotification,
    ) => NotificationResponse | undefined,
  ) {
    return _subscribe(
      { type: NotificationType.NOTIFY_ON_PROCESS_CHANGED },
      (notification) => {
        switch (notification?.type?.case) {
          case "processChangeNotification":
            return handler(notification.type.value);
          default:
            break;
        }

        return { unsubscribe: false };
      },
    );
  },
};

export const promptDidReturn = {
  subscribe(
    handler: (
      notification: ShellPromptReturnedNotification,
    ) => NotificationResponse | undefined,
  ) {
    return _subscribe(
      { type: NotificationType.NOTIFY_ON_PROMPT },
      (notification) => {
        switch (notification?.type?.case) {
          case "shellPromptReturnedNotification":
            return handler(notification.type.value);
          default:
            break;
        }

        return { unsubscribe: false };
      },
    );
  },
};

export const historyUpdated = {
  subscribe(
    handler: (
      notification: HistoryUpdatedNotification,
    ) => NotificationResponse | undefined,
  ) {
    return _subscribe(
      { type: NotificationType.NOTIFY_ON_HISTORY_UPDATED },
      (notification) => {
        switch (notification?.type?.case) {
          case "historyUpdatedNotification":
            return handler(notification.type.value);
          default:
            break;
        }

        return { unsubscribe: false };
      },
    );
  },
};

export async function insert(
  text: string,
  request?: Omit<TextUpdate, "insertion" | "$typeName">,
  terminalSessionId?: string,
) {
  if (request) {
    return sendInsertTextRequest({
      terminalSessionId,
      type: {
        case: "update",
        value: create(TextUpdateSchema, { ...request, insertion: text }),
      },
    });
  }
  return sendInsertTextRequest({
    terminalSessionId,
    type: { case: "text", value: text },
  });
}
